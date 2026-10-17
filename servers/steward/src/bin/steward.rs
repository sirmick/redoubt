//! The steward, the program: read the manifest lines from the arguments, boot the policy core,
//! carve each principal's budgets under `users`, say the tree on the console, then serve the
//! steward's protocol on the endpoint named `steward` until it is destroyed.
//!
//! What it decides is in `redoubt-steward-server`'s library, which host tests drive; this is the
//! machine's side of [`Kernel`]: the kernel calls, the binding table (which server each shared
//! slot of a session's namespace is, for which domain, and where the child finds it), the launch
//! through the loader stub, and a watcher per session for its exit notice.
//!
//! **The binding table** (servers/steward.md, "Two embedders and a reference";
//! `redoubt_steward_server::own::binding`, host-tested), by slot:
//!
//! | Slot | Server | Unlabelled session | Vault session |
//! | --- | --- | --- | --- |
//! | 0 | `bootfsd` | `/boot` and handle `bootfsd` | the same |
//! | 1 | the home volume's server, rooted at the home, through the principal's one carve with its quota | at the home's path | the same; the server refuses its writes (R25) |
//! | 2 | the label set's volume's server | nothing | `/vault` |
//! | 3 | `ipd`, granted the principal's scope | `/net` | nothing |
//! | 4 | the console: `sshd`'s channel, or `consoled` for the UART's session | `/dev/cons` | the same |
//! | 5 | the system volume's `erofsd` | handle `erofsd:system`, named by `endpoint=` | the same |
//!
//! A slot bound to nothing makes no call and gives the child no entry, so its path is `:enoent`.
//!
//! **Cleanup at the loop's turn** (servers/steward.md, "Failure and restart"): an end batch is
//! answered once `DestroyBudget` has run, the cutoff; each connection it gave back is a step in
//! the library's queue (`redoubt_steward_server::cleanup`), and the loop runs the steps due at
//! its top, before it receives again, each within [`CLEANUP_US`]: a `disconnect` by id at the
//! server that minted it, or `ended` on `sshd`'s channel console. A step a busy or recovering
//! server does not take in time ends the turn (the rest stay due for the next, after one
//! receive), is retried at a later turn and parked after the cap with a line, until a call of
//! the steward's at that server is answered or the server restarts; the loop never waits more
//! than one such bound on a server while it serves nobody.
//!
//! **The console lines at the loop's turn** (the same page): every line the steward says is
//! queued ([`Console`], the library's `redoubt_steward_server::console`) and written through one
//! fid kept open at `consoled`, a line one write within [`CONSOLE_US`], at two points: before a
//! call's reply, so a call's records come before anything its caller says of the answer, and at
//! the loop's top after the cleanup steps, for every other event's lines. Either turn stops at
//! the first line the console did not take in time, the rest queued in order for the next.

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

#[cfg(target_os = "none")]
mod machine {
    use alloc::collections::BTreeMap;
    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::num::NonZeroU64;
    use core::sync::atomic::{AtomicU32, Ordering};

    use redoubt_client::file::{Connection, ROOT};
    use redoubt_client::launch::Launch;
    use redoubt_client::typed;
    use redoubt_rt::abi::{BudgetSpec, Error, FOREVER, Handle, MAX_LEND_PAGES};
    use redoubt_rt::client::{ClientError, Connection as Nine, Lend};
    use redoubt_rt::handle::{Budget, Endpoint, close};
    use redoubt_rt::ipc::{Buffer, Event, Request};
    use redoubt_rt::server::ninep::{DMDIR, mode};
    use redoubt_rt::server::typed::finish;
    use redoubt_rt::server::{close_delivery, own_args};
    use redoubt_rt::startup::Startup;
    use redoubt_rt::wire::proto::{consol, consrelay, crossing, ipd, steward};
    use redoubt_steward::consts::CROSSING_LIFE;
    use redoubt_steward::domain::Domain;
    use redoubt_steward::effect::{Object, Output};
    use redoubt_steward::event::EventKind;
    use redoubt_steward::inspect;
    use redoubt_steward_server::approval::{Line, Lines, parse};
    use redoubt_steward_server::cleanup::TRIES;
    use redoubt_steward_server::console::{Queue, Sink};
    use redoubt_steward_server::own::{
        Bound, Child, How, IPD, Own, SYSTEM, binding, lease_args, session_args,
    };
    use redoubt_steward_server::protocol::{APPROVAL, Serving, answer_with, watches as watches_call};
    use redoubt_steward_server::watchers::Watchers;
    use redoubt_steward_server::{Kernel, SLOTS, Steward, start};

    /// The stub's own flat binary (`build.rs`).
    static STUB_BIN: &[u8] = include_bytes!(env!("STUB_BIN"));

    /// The startup block named no `users` budget or no `steward` endpoint.
    pub const NO_HANDLE: u32 = 2;
    /// The manifest lines were refused, `users` was not empty, or a carve failed: the box has no
    /// users ([`redoubt_steward_server::StartError`], said on the console first).
    pub const NOT_STARTED: u32 = 3;
    /// The core exited on an event its embedder's guarantee excludes: `init` restarts the steward.
    pub const CORE_EXITED: u32 = 4;
    /// Test-only, for the bench's `steward-restart` and `steward-restart-ssh` (feature
    /// `restart-probe`, off in every default build, as `littlefsd`'s and `netd`'s are): every
    /// instance serves as usual and exits with this code [`PROBE_DELAY`] µs after it starts its
    /// console session, so `init` empties `users` and restarts it again and again, each restart
    /// far enough from the last to stay under the reboot rule (more than 5 within 60 seconds).
    #[cfg(feature = "restart-probe")]
    pub const PROBE_EXIT: u32 = 9;
    /// The `watch` calls held at once: one `sshd`'s, and a restarted `sshd`'s before the first's
    /// abandonment notice arrives.
    const WATCHES: usize = 2;
    #[cfg(feature = "restart-probe")]
    const PROBE_DELAY: u64 = 14_000_000;
    /// Test-only, for the bench's `steward-latency` (feature `latency-probe`, off in every
    /// default build; kernel/scheduling.md, "Responsiveness"): a lease asked for exactly
    /// [`LATENCY_LEASE_US`] is the probe's. After its `start_agent` is answered the steward prints
    /// the answer to its next receive as a `reply_wait` sample (its own work after the reply),
    /// arms there a timeout of [`LATENCY_TIMER_US`] and prints its wake as a `timer_wake` sample (`time_now`
    /// less the deadline it armed), then one of [`LATENCY_DECIDE_US`] whose wake is a
    /// `decision_wake` sample, on which it ends the lease itself (the core's `EndLease` on the
    /// badge that started it, no new row or effect) and prints its `budget_destroy` call to
    /// return as `destroy_call`; a lease the kernel's deadline took prints its exit notice's
    /// arrival less the deadline as `deadline_notice`. Each sample's group is the count of the
    /// live leases, itself among them (`N=2`); the counts go out as `LATENCY-COUNT` lines when
    /// the leases are all gone and the case has written the round's marker ([`LATENCY_ROUND`]
    /// and the round's number) in the console principal's home, and after the last round's
    /// ([`LATENCY_ROUNDS`]), `LATENCY-END` with the steward's clock, the window's end. A steward that finds
    /// [`LATENCY_DONE`] in the console
    /// principal's home at its start exits [`LATENCY_EXIT`] at once: six such exits trip init's
    /// reboot rule, whose reset dumps the kernel's trace the bench's oracle reads.
    #[cfg(feature = "latency-probe")]
    const LATENCY_LEASE_US: u64 = 9_000_000;
    #[cfg(feature = "latency-probe")]
    const LATENCY_TIMER_US: u64 = 20_000;
    #[cfg(feature = "latency-probe")]
    const LATENCY_DECIDE_US: u64 = 30_000;
    #[cfg(feature = "latency-probe")]
    const LATENCY_DONE: &str = "latency-done";
    #[cfg(feature = "latency-probe")]
    const LATENCY_ROUND: &str = "latency-round-";
    /// The case's rounds: N = 1, 2 and 3 live leases.
    #[cfg(feature = "latency-probe")]
    const LATENCY_ROUNDS: usize = 3;
    #[cfg(feature = "latency-probe")]
    const LATENCY_POLL_US: u64 = 500_000;
    #[cfg(feature = "latency-probe")]
    pub const LATENCY_EXIT: u32 = 9;

    /// The program a session runs, on `/boot`, and its start module.
    const PROGRAM: &str = "beamlet";
    /// A context's console relay, on `/boot` (servers/consrelay.md).
    const RELAY: &str = "consrelay";
    /// A crossing's reader or writer of one item, on `/boot` (servers/steward.md,
    /// "Declassification and push"), the names its startup block holds (servers/crossing), and
    /// its first-thread stack and heap cap in pages: it holds one page of lend, one fid and at
    /// most `DECLASSIFY_MAX + 1` bytes read; a write's bytes stay in the steward's lend.
    const CROSSING: &str = "crossing";
    const CROSSING_SERVE: &str = "serve";
    const CROSSING_VOLUME: &str = "volume";
    const CROSSING_STACK_PAGES: usize = 4;
    const CROSSING_HEAP_PAGES: u32 = 4;
    /// The badge the steward's call right on a crossing's endpoint carries: the one caller.
    const CROSSING_CALL: u64 = 1;
    /// The name the relay finds its hello badge under (servers/consrelay.md).
    const RELAY_HELLO: &str = "hello";
    /// The relay's first-thread stack and its heap cap, in pages: twice what `consrelay-footprint`
    /// measured, 8,360 bytes of stack and a 22-page heap, which holds its buffers from its start.
    const RELAY_STACK_PAGES: usize = 5;
    const RELAY_HEAP_PAGES: u32 = 44;
    /// What a session's budget holds for its relay, and so what its VM is not given: the relay's
    /// image, its four stacks, its heap cap and what the kernel charges for the process, rounded
    /// up to 128 (docs/kernel/budgets.md). The console session has no relay and gets no more.
    const RELAY_PAGES: u64 = 128;
    /// How long a relay has to say hello after its launch, and to answer `attach` or `detach`
    /// (µs): the steward never waits without bound on code in a context's budget.
    const HELLO_US: u64 = 2_000_000;
    const RELAY_US: u64 = 1_000_000;
    /// `label-probe` only: how long after an attach the probe hands the channel's console to
    /// another relay (µs): `sshd` is free again once the login is answered, and the channel's own
    /// relay has opened its console by then.
    #[cfg(feature = "label-probe")]
    const PROBE_SETTLE_US: u64 = 3_000_000;
    const SHELL: &str = "Elixir.Redoubt.Shell";
    /// The module a lease's VM runs: the agent of this package (servers/steward.md, "Leases";
    /// userland/agents.md, "Leases"), from the system volume as the shell is.
    const AGENT: &str = "Elixir.Redoubt.Agents.Echo";
    /// A session VM's first-thread stack, in pages: twice beamlet's largest measured peak,
    /// 36,136 bytes at the idle prompt (`beamlet-footprint`; a compile at the prompt peaks lower,
    /// `beamlet-compile-footprint`: servers/init.md, "Stacks"; docs/testbench.md, "The memory
    /// budget"). beamlet gives its other schedulers the same (`SCHEDULER_STACK_PAGES`,
    /// userland/otp/redoubt/src/lib.rs).
    const SESSION_STACK_PAGES: usize = 18;
    /// A session VM's schedulers (`schedulers=N`, docs/userland/beamlet.md, "beamlet on
    /// Redoubt"): the image's choice, since the kernel does not tell a program how many harts
    /// there are. Each after the first is a thread with a stack of `SESSION_STACK_PAGES`, as
    /// beamlet sizes it, and its thread page.
    const SESSION_SCHEDULERS: usize = 2;
    /// The pages a session VM's locks may make endpoints of: one for each of its two locks and
    /// its idle schedulers' wake-up, and one for each scheduler that waits (beamlet's runtime
    /// makes a lock's endpoint the first time a thread must wait on it).
    const SESSION_LOCK_PAGES: usize = 3 + SESSION_SCHEDULERS;
    /// A root badge of the steward's own, below the minted range and none of the protocol's
    /// roles: each session's watcher reports its exit on it.
    const EXITS: u64 = 4;
    /// A watcher's stack, in pages, as `init`'s.
    const WATCH_STACK_PAGES: usize = 4;
    /// The badge the steward sends a session's exit endpoint to an idle watcher under.
    const WORK: u64 = 1;
    /// The badge of the steward's send right on a session's notices endpoint, and the name the
    /// VM finds the receive right under (userland/sessions.md, "What a session is told").
    const NOTICES: u64 = 1;
    const NOTICES_HANDLE: &str = "notices";
    /// How long a `lease_ended` send waits for the session's VM to take it (µs): past it the
    /// notice is dropped, and the steward is never held by a session that does not read.
    const NOTICE_US: u64 = 100_000;
    /// How long a cleanup step waits for its server to take the call (µs): the steward's own
    /// send bound, [`NOTICE_US`], never the client library's call timeout (`RELEASE_TIMEOUT`, a
    /// second). A server busy or recovering past it is retried at a later turn
    /// (`redoubt_steward_server::cleanup`).
    const CLEANUP_US: u64 = NOTICE_US;
    /// How long a console line's write waits for `consoled` to take it (µs): the same bound, for
    /// the same reason; a line it did not take stays queued for a later turn
    /// (`redoubt_steward_server::console`).
    const CONSOLE_US: u64 = NOTICE_US;
    /// The names a cleanup step's server goes by in the queue where the steward reaches it by no
    /// handle name: its own console's server, and `sshd` for a channel console told `ended`.
    const CONSOLED_NAME: &str = "consoled";
    const SSHD_NAME: &str = "sshd";
    /// Open approval channels at most, and so reader threads at most (servers/steward.md, "The
    /// powerbox and approvals"): the steward's bound on its own threads beside the watchers,
    /// declared as theirs is. An open past it is refused, and the person's `ssh approve@box`
    /// fails.
    const CHANNELS: usize = 4;
    /// A reader's stack, in pages: it attaches a 9P connection, reads, parses a line and makes
    /// one typed call, the same depth as `sshd`'s `say`.
    const READER_STACK_PAGES: usize = 8;
    /// The badge the steward hands a reader its channel under, on the readers' work endpoint,
    /// and how long the hand-over waits for a reader to take it (µs): an idle reader is waiting
    /// already, a new one is about to; past the bound the open fails.
    const HAND: u64 = 1;
    const HAND_US: u64 = 1_000_000;
    /// A root badge of the steward's own, below the minted range and none of the protocol's
    /// roles: each reader reports a channel's end on it, and is counted idle again.
    const READER_DONE: u64 = 5;
    /// How long a screen's write waits for `sshd`'s channel to take it (µs): past it the screen
    /// is dropped and the person asks again with `pending`, so the main loop is never held by a
    /// terminal that does not read, and a reader's call on badge `APPROVAL` is always served:
    /// the pair cannot deadlock.
    const SCREEN_US: u64 = 1_000_000;
    /// The steward's lend, in pages: the most a call may lend, so a 64-page batch of a session's
    /// image is read from `bootfsd` in four reads, not sixteen or more (a 9P read carries at most
    /// the lend's `iounit`, and a call costs about the same whatever it carries).
    const LEND_PAGES: usize = MAX_LEND_PAGES;

    /// The handle the watchers report on: `EXITS` minted on the steward's endpoint.
    static REPORT: AtomicU32 = AtomicU32::new(0);
    /// The handle the readers report on: `READER_DONE` minted on the steward's endpoint.
    static READER_REPORT: AtomicU32 = AtomicU32::new(0);

    fn h(index: u32) -> Handle { Handle::new(index).expect("a slot is at least 1") }

    /// A watcher (`redoubt_steward_server::watchers`): `arg` is the work endpoint's handle. It
    /// takes a session's exit endpoint from there, waits for its one exit notice, reports the
    /// endpoint on [`REPORT`], and takes the next; a thread's stack is never given back, so the
    /// thread is reused.
    extern "C" fn watch(arg: usize) -> ! {
        let work = Endpoint::from_handle(h(arg as u32));
        loop {
            let index = match work.receive(FOREVER, 0) {
                Ok(Event::Send(delivery)) => {
                    close_delivery(&delivery);
                    delivery.words[0] as u32
                }
                Ok(Event::Call(request)) => {
                    drop(request);
                    continue;
                }
                Ok(_) => continue,
                Err(_) => redoubt_rt::handle::thread_exit(),
            };
            let exit = Endpoint::from_handle(h(index));
            // The notice's cause and code go with the report: an agent's VM has no console of
            // its own to say how it ended.
            let (cause, code) = loop {
                match exit.receive(FOREVER, 0) {
                    Ok(Event::Exit(n)) => break (n.cause as u64, u64::from(n.code)),
                    Err(_) => break (0, 0),
                    Ok(Event::Call(request)) => drop(request),
                    Ok(Event::Send(delivery)) => close_delivery(&delivery),
                    Ok(_) => {}
                }
            };
            let report = Endpoint::from_handle(h(REPORT.load(Ordering::Acquire)));
            let _ = report.send(&[u64::from(index), cause, code, 0], &[], None, FOREVER);
        }
    }

    /// What a connection the steward holds for a child was minted through: a server it was
    /// handed, by handle name, a connection it carved and keeps, by its key, or its own console.
    enum Via {
        Server(String),
        Carved(String),
        Console,
    }

    /// An approval channel's reader (servers/steward.md, "The powerbox and approvals"): `arg` is
    /// the readers' work endpoint. It takes a channel from there (the channel's id, the reader
    /// console `sshd` minted for it, and a send right of the steward's own badge `APPROVAL`),
    /// blocks in `read` on the console for the person's lines, parses each
    /// (`redoubt_steward_server::approval`) and calls the steward on that badge, which the main
    /// loop serves as any call (it never waits on a reader: its screen writes are bounded by
    /// [`SCREEN_US`]); the answer is written back on the same console. The channel's end ends
    /// its input; the reader reports it done on [`READER_REPORT`] and takes the next channel: a
    /// thread's stack is never given back, so readers are reused as the watchers are. A new one
    /// starts only when every reader is busy, and a reader counts busy until its report is
    /// taken, so there are at most [`CHANNELS`] readers plus those between a channel's end and
    /// the main loop's taking of its report.
    extern "C" fn reader_thread(arg: usize) -> ! {
        let work = Endpoint::from_handle(h(arg as u32));
        loop {
            let (channel, console, steward) = match work.receive(FOREVER, 0) {
                Ok(Event::Send(d)) if d.caller.badge == HAND => {
                    let handles = d.handles.as_slice();
                    if let [Some(console), Some(steward)] = handles {
                        // The channel's id in two machine words: a word is 32 bits on rv32.
                        (d.words[0] | (d.words[1] << 32), *console, *steward)
                    } else {
                        close_delivery(&d);
                        continue;
                    }
                }
                Ok(Event::Send(d)) => {
                    close_delivery(&d);
                    continue;
                }
                Ok(_) => continue,
                Err(_) => redoubt_rt::handle::thread_exit(),
            };
            read_channel(channel, console, steward);
            let _ = close(console);
            let _ = close(steward);
            let report = Endpoint::from_handle(h(READER_REPORT.load(Ordering::Acquire)));
            let _ = report.send(&[channel & 0xffff_ffff, channel >> 32, 0, 0], &[], None, FOREVER);
        }
    }

    /// The reader's loop, until the console's end of input or a failure of the connection.
    fn read_channel(channel: u64, console: Handle, steward: Handle) {
        let Ok(mut lend) = Lend::new(1) else { return };
        let Ok(conn) = Connection::attach(Endpoint::from_handle(console), &mut lend) else { return };
        let Ok(file) = conn.open(&mut lend, "", mode::ORDWR) else { return };
        let steward = Endpoint::from_handle(steward);
        let mut lines = Lines::new();
        let mut buf = [0u8; 256];
        loop {
            let n = match file.read_at(&mut lend, 0, &mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            let typed = lines.push(&buf[..n]);
            for _ in 0..lines.dropped() {
                let _ = file.write_at(&mut lend, 0, b"line too long\r\n");
            }
            for line in typed {
                let answer = match parse(&line) {
                    None => String::from("unknown command\r\n"),
                    Some(Line::Pending) => {
                        let m = steward::Message::Pending(steward::Pending { channel });
                        answer_line(&steward, &mut lend, &m, "")
                    }
                    Some(Line::Approve { request, hash }) => {
                        let m = steward::Message::Approve(steward::Approve {
                            channel,
                            request,
                            hash: hash.bytes(),
                        });
                        answer_line(&steward, &mut lend, &m, &format!("approved {request:016x}\r\n"))
                    }
                    Some(Line::Deny { request }) => {
                        let m = steward::Message::Deny(steward::Deny { channel, request });
                        answer_line(&steward, &mut lend, &m, &format!("denied {request:016x}\r\n"))
                    }
                };
                if !answer.is_empty() {
                    let _ = file.write_at(&mut lend, 0, answer.as_bytes());
                }
            }
        }
    }

    /// The steward's answer to a reader's call as a line: `ok` on success, else the refusal by
    /// the wire's name (`refused: hash_mismatch`), or that the steward did not answer.
    fn answer_line(steward: &Endpoint, lend: &mut Lend, m: &steward::Message<'_>, ok: &str) -> String {
        match typed::call::<steward::Protocol, _>(steward, lend, m, &[], |_, _| Some(())) {
            Ok(Some(())) => String::from(ok),
            Err(redoubt_client::Error::Server(code)) => match steward::ErrorCode::from_code(code) {
                Some(c) => format!("refused: {}\r\n", snake(&format!("{c:?}"))),
                None => format!("refused: {code}\r\n"),
            },
            _ => String::from("the steward did not answer\r\n"),
        }
    }

    /// `HashMismatch` as the wire's table names it, `hash_mismatch`.
    fn snake(name: &str) -> String {
        let mut out = String::new();
        for c in name.chars() {
            if c.is_ascii_uppercase() && !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        }
        out
    }

    /// What the steward holds for a child, under a key of its own ([`Machine::held`]).
    struct Held {
        /// The steward's own copy of the handle: closed once a launch has copied it into the
        /// child, since the kernel ends a connection minted in a scope at the scope's end (R41)
        /// and frees its index for the next handle installed, so a copy kept would soon name
        /// something else. Kept only for what the steward itself still uses: a channel's
        /// console, told `ended`; a relay's control connection.
        handle: Option<Handle>,
        /// What it was minted through and the id it is disconnected by there; none for one the
        /// steward did not make (a badge, `sshd`'s console).
        via: Option<(Via, u64)>,
        /// `sshd`'s channel console: told `ended` when it is released.
        console: bool,
    }

    struct Machine<'s> {
        startup: &'s Startup<'s>,
        /// The steward's console: the lines it says, and the fid they go through.
        out: &'s mut Console,
        lend: Lend,
        own: Endpoint,
        /// Each server the steward was handed, attached once, by its handle name.
        servers: BTreeMap<String, Connection>,
        /// The steward's own console, from which the UART session's console is minted.
        consoled: Option<Connection>,
        /// The next login's consoles: `sshd`'s channel connection, the steward's, and the one its
        /// relay is given.
        console: Option<Handle>,
        relay_console: Option<Handle>,
        /// What is held for children, by a key drawn once and never reused: the core's `Handle`.
        held: BTreeMap<u64, Held>,
        next_key: u64,
        /// The connections carved once with a quota and kept for the steward's life, by key (a
        /// principal's home): every session's is minted through one with no quota of its own, so
        /// they share it, and the handle name of the server it is at. `init` disconnects them at
        /// the steward's exit, with all minted below; a server's restart forgets its
        /// ([`Machine::reconnected`]), and the next carve mints them again.
        carved: BTreeMap<String, (Handle, String)>,
        /// The kept carves the steward itself reads and writes files through, attached once, by
        /// the carve's key: a push's source, a declassified item's landing (servers/steward.md,
        /// "Declassification and push"). Forgotten with the carve at the server's restart.
        homes: BTreeMap<String, Connection>,
        /// The connection `init` handed the steward at a server that restarted since, by the
        /// server's handle name, in place of the startup block's (servers/steward.md, "Failure
        /// and restart").
        replaced: BTreeMap<String, Handle>,
        /// Each principal's account and name.
        accounts: Vec<(u64, String)>,
        own_lines: Own,
        /// A session's pages, the manifest's `sizes`: its VM's and its relay's; and a lease's,
        /// with the bytes of its sponsor's home its root is carved with.
        session_pages: u64,
        agent_pages: u64,
        agent_quota: u64,
        /// The program's length on `/boot`, the relay's and the crossing program's, and a
        /// crossing budget's pages, the manifest's `sizes.crossing`.
        program_len: usize,
        relay_len: usize,
        crossing_len: usize,
        crossing_pages: u64,
        /// The endpoint relays say hello on, and the badge the last one was given.
        hello: Option<Endpoint>,
        hellos: u64,
        /// The VM's console the last relay served, for the next console slot.
        relay_vm: Option<Handle>,
        /// `label-probe` only: the first labelled context's relay (its control connection),
        /// which every later attach of another relay hands its channel's console too.
        #[cfg(feature = "label-probe")]
        probe_victim: Option<Handle>,
        /// `label-probe` only: the hand-over due, at this time, of this channel console.
        #[cfg(feature = "label-probe")]
        probe_due: Option<(u64, Handle)>,
        /// The watchers' work endpoint, and the badge the steward sends them work on.
        work: Endpoint,
        work_send: Endpoint,
        watchers: Watchers,
        /// The next approval channel's consoles, the two connections `sshd` sent with its open:
        /// the steward's, for screens, and its reader's.
        approval: Option<(Handle, Handle)>,
        /// Each open approval channel's screen console, by the channel's id.
        channels: BTreeMap<u64, Handle>,
        /// The readers' work endpoint, the badge the steward hands them channels on, and the
        /// pool's count (reused as the watchers are).
        reader_work: Endpoint,
        reader_hand: Endpoint,
        readers: Watchers,
        /// `latency-probe` only: the last `budget_destroy`'s end and length (µs), the probe's
        /// `destroy_call` sample.
        #[cfg(feature = "latency-probe")]
        last_destroy: Option<(u64, u64)>,
    }

    impl Machine<'_> {
        /// Test-only, for the bench's `steward-context-labels` (feature `label-probe`, off in
        /// every default build): a broken embedder that also hands the labelled context's relay
        /// the console of every other context's channel, here the steward's own connection,
        /// [`PROBE_SETTLE_US`] after the attach, from the serving loop, once `sshd` has served
        /// the channel's own relay. `sshd` refuses the labelled relay's calls there, by admission
        /// (the channel's console admits the platform and one account and label set) or by its
        /// label check (R25), so nothing it keeps reaches that channel, and the channel's own
        /// context still reaches it.
        #[cfg(feature = "label-probe")]
        fn probe_cross(&mut self, relay: Handle, console: Handle) {
            if self.probe_victim.is_some_and(|v| v != relay) {
                let now = redoubt_rt::handle::time_now().unwrap_or(0);
                self.probe_due = Some((now.saturating_add(PROBE_SETTLE_US), console));
            }
        }

        /// The hand-over [`Machine::probe_cross`] set, now that it is due.
        #[cfg(feature = "label-probe")]
        fn probe_hand(&mut self) {
            let (Some(victim), Some((_, console))) = (self.probe_victim, self.probe_due.take()) else {
                return;
            };
            let m = consrelay::Message::Attach(consrelay::Attach { note: "" });
            let told = typed::call_within::<consrelay::Protocol, _>(
                &Endpoint::from_handle(victim),
                &mut self.lend,
                &m,
                &[console],
                RELAY_US,
                |_, _| (),
            );
            let line = format!(
                "steward: label-probe: the labelled relay was handed another channel: {}\n",
                told.is_ok()
            );
            self.out.say(&line);
        }

        /// The handle the steward reaches the server named `name` by: the one `init` handed it
        /// since the server's last restart, else the startup block's.
        fn named(&self, name: &str) -> Option<Handle> {
            self.replaced.get(name).copied().or_else(|| self.startup.handle(name))
        }

        fn server(&mut self, name: &str) -> Result<Connection, Error> {
            if let Some(conn) = self.servers.get(name) {
                return Ok(conn.clone());
            }
            let handle = self.named(name).ok_or(Error::BadHandle)?;
            let conn = Connection::attach(Endpoint::from_handle(handle), &mut self.lend)
                .map_err(|_| Error::Refused)?;
            self.servers.insert(name.into(), conn.clone());
            Ok(conn)
        }

        /// The server the steward was handed as `name`, unattached: minting and disconnecting
        /// need no fid, and the steward may not read a labelled volume's root itself (R25).
        fn unattached(&self, name: &str) -> Result<Nine, Error> {
            let handle = self.named(name).ok_or(Error::BadHandle)?;
            Ok(Nine::new(Endpoint::from_handle(handle)))
        }

        /// A fresh connection to `name`'s server rooted at `root`, minted in `scope`, held until
        /// released.
        fn fresh(&mut self, name: &str, root: &str, scope: Handle) -> Result<Option<u64>, Error> {
            let server = self.unattached(name)?;
            let scope = Budget::from_handle(scope);
            let (endpoint, id) =
                server.new_connection(&mut self.lend, root, 0, Some(&scope)).map_err(minted)?;
            Ok(Some(self.hold(endpoint.handle(), Some((Via::Server(name.into()), id)), false)))
        }

        /// The connection kept for `key`, carved at `name`'s server rooted at `root` with `quota`
        /// bytes the first time it is needed.
        fn carve(&mut self, key: &str, name: &str, root: &str, quota: u64) -> Result<Nine, Error> {
            if let Some((handle, _)) = self.carved.get(key) {
                return Ok(Nine::new(Endpoint::from_handle(*handle)));
            }
            let server = self.unattached(name)?;
            let (endpoint, _) = server.new_connection(&mut self.lend, root, quota, None).map_err(minted)?;
            self.carved.insert(key.into(), (endpoint.handle(), name.into()));
            Ok(Nine::new(endpoint))
        }

        /// A fresh connection at `below` the root of the one kept for `key`, minted in `scope`,
        /// held until released: at the kept one's own root with `below_quota` 0, sharing its
        /// quota; below it with `below_quota` bytes carved from the kept one's, given back when it
        /// is released (walfsd.md, "Quotas").
        fn fresh_carved(
            &mut self,
            key: &str,
            name: &str,
            root: &str,
            quota: u64,
            below: &str,
            below_quota: u64,
            scope: Handle,
        ) -> Result<Option<u64>, Error> {
            let kept = self.carve(key, name, root, quota)?;
            let scope = Budget::from_handle(scope);
            let (endpoint, id) =
                kept.new_connection(&mut self.lend, below, below_quota, Some(&scope)).map_err(minted)?;
            Ok(Some(self.hold(endpoint.handle(), Some((Via::Carved(key.into()), id)), false)))
        }

        fn principal(&self, domain: &Domain) -> Option<String> {
            let a = domain.account().get();
            self.accounts.iter().find(|(acc, _)| *acc == a).map(|(_, n)| n.clone())
        }

        /// The steward's own file connection at `principal`'s home: the kept carve, attached once
        /// (its fids are the steward's alone; the carve mints connections and needs none).
        fn home(&mut self, principal: &str) -> Result<Connection, Error> {
            let h = self.own_lines.home(principal).cloned().ok_or(Error::BadHandle)?;
            let key = format!("home {principal}");
            if let Some(conn) = self.homes.get(&key) {
                return Ok(conn.clone());
            }
            let kept = self.carve(&key, &h.handle, &h.path, h.quota)?;
            let conn =
                Connection::attach(kept.into_endpoint(), &mut self.lend).map_err(|_| Error::Refused)?;
            self.homes.insert(key, conn.clone());
            Ok(conn)
        }

        /// Reads `buf.len()` bytes of the program `path` at `at`, through `bootfsd`.
        fn read_program(&mut self, path: &str, at: usize, buf: &mut [u8]) -> Result<(), Error> {
            let conn = self.server("bootfsd")?;
            let file = conn.open(&mut self.lend, path, mode::OREAD).map_err(|_| Error::Refused)?;
            let mut done = 0;
            while done < buf.len() {
                let read = file.read_at(&mut self.lend, (at + done) as u64, &mut buf[done..]);
                match read {
                    Ok(0) | Err(_) => break,
                    Ok(n) => done += n,
                }
            }
            let _ = file.close(&mut self.lend);
            if done == buf.len() { Ok(()) } else { Err(Error::InvalidArgument) }
        }

        /// Holds `handle` for a child under a new key, with what disconnects it, if anything,
        /// and whether it is a channel's console.
        fn hold(&mut self, handle: Handle, via: Option<(Via, u64)>, console: bool) -> u64 {
            self.next_key += 1;
            self.held.insert(self.next_key, Held { handle: Some(handle), via, console });
            self.next_key
        }

        /// Keeps `handle` until it is released, with nothing to disconnect.
        fn keep(&mut self, handle: Handle) -> u64 { self.hold(handle, None, false) }

        /// The handle held under `key`, if the steward still has its copy.
        fn handle_of(&self, key: u64) -> Option<Handle> { self.held.get(&key).and_then(|h| h.handle) }

        /// Has a watcher wait for the one exit notice on `exit`: an idle one, or a new one when
        /// every watcher is watching. Unwatched, the notice would never be read, so a launch
        /// that cannot be watched is a failed step, as a refused one is.
        fn watch_exit(&mut self, exit: Handle) -> Result<(), Error> {
            let new = self.watchers.watch();
            let watched = if new {
                Buffer::new(WATCH_STACK_PAGES)
                    .and_then(|stack| {
                        redoubt_rt::handle::thread_create(watch, stack, self.work.handle().index() as usize)
                    })
                    .map(|_| ())
            } else {
                Ok(())
            }
            .and_then(|()| {
                self.work_send
                    .send(&[u64::from(exit.index()), 0, 0, 0], &[], None, FOREVER)
                    .map_err(|(e, _)| e)
            });
            if watched.is_err() {
                self.watchers.unwatch(new);
            }
            watched
        }

        /// The relay launched last says hello on `badge`: the VM's console and the control
        /// connection, or nothing within [`HELLO_US`].
        fn hello(&mut self, badge: u64) -> Result<(Handle, Handle), Error> {
            let hello = self.hello.as_ref().ok_or(Error::BadHandle)?;
            let deadline = redoubt_rt::handle::time_now()?.saturating_add(HELLO_US);
            loop {
                let wait = deadline.saturating_sub(redoubt_rt::handle::time_now()?);
                if wait == 0 {
                    return Err(Error::Timeout);
                }
                let Event::Send(d) = hello.receive(wait, 0)? else { continue };
                let said =
                    matches!(consrelay::Message::decode(&d.words, &[], 2), Ok(consrelay::Message::Hello(_)));
                let handles = d.handles.as_slice();
                if let (true, true, [Some(vm), Some(control)]) = (d.caller.badge == badge, said, handles) {
                    return Ok((*vm, *control));
                }
                // An earlier relay's hello, too late: what it brought is closed.
                close_delivery(&d);
            }
        }
    }

    impl Kernel for Machine<'_> {
        type Budget = Handle;
        /// A key into [`Machine::held`], never a handle's index.
        type Handle = u64;

        /// A relay's VM console not taken by its own batch's console slot, because a step between
        /// failed, is closed here: every batch that launches a VM creates its budget and scope
        /// before its relay, so it never reaches another session's console slot, the UART
        /// session's included.
        fn create(&mut self, parent: Handle, spec: &BudgetSpec) -> Result<Handle, Error> {
            if let Some(stale) = self.relay_vm.take() {
                let _ = close(stale);
            }
            Budget::from_handle(parent).create_child(spec).map(|b| b.handle())
        }

        fn empty(&mut self, budget: Handle) -> Result<bool, Error> {
            let u = Budget::from_handle(budget).usage()?;
            Ok(u.pages_usage == 0 && u.processes_usage == 0 && u.weight_carved == 0)
        }

        fn destroy(&mut self, budget: Handle) -> Result<(), Error> {
            #[cfg(feature = "latency-probe")]
            let began = redoubt_rt::handle::time_now().unwrap_or(0);
            let destroyed = Budget::from_handle(budget).destroy();
            #[cfg(feature = "latency-probe")]
            {
                let end = redoubt_rt::handle::time_now().unwrap_or(began);
                self.last_destroy = Some((end, end.saturating_sub(began)));
            }
            destroyed
        }

        fn budget_id(&self, budget: Handle) -> u64 { u64::from(budget.index()) }

        fn mint(&mut self, badge: u64, stamp: Handle) -> Result<u64, Error> {
            let badge = NonZeroU64::new(badge).ok_or(Error::InvalidArgument)?;
            let minted = self.own.mint(badge, Some(&Budget::from_handle(stamp)))?;
            Ok(self.keep(minted.handle()))
        }

        /// Every connection a server mints for the child is minted in `scope` (R41): a 9P
        /// server's by `new_connection`'s scope, `ipd`'s by its typed `grant`'s.
        fn connect(&mut self, owner: &Object, slot: u16, scope: Handle) -> Result<Option<u64>, Error> {
            let principal = self.principal(&owner.domain).ok_or(Error::BadHandle)?;
            let labels = owner.domain.labels().as_slice();
            let child = Child::of(owner.kind, labels);
            let Some(bound) = binding(&self.own_lines, &principal, labels, child, slot, self.agent_quota)
            else {
                return Ok(None);
            };
            match bound.how {
                How::Fresh { server, root } => self.fresh(&server, &root, scope),
                How::Carved { key, server, root, quota, below, below_quota } => {
                    self.fresh_carved(&key, &server, &root, quota, &below, below_quota, scope)
                }
                How::Grant { scope: net } => {
                    let conn = self.server(IPD)?;
                    let grant = ipd::Message::Grant(ipd::Grant { scope: &net });
                    let made = typed::call::<ipd::Protocol, _>(
                        conn.endpoint(),
                        &mut self.lend,
                        &grant,
                        &[scope],
                        |r, got| match r {
                            ipd::Reply::Grant(g) => got.take(0).map(|h| (h, g.id)),
                            _ => None,
                        },
                    );
                    let (handle, id) = made.ok().flatten().ok_or(Error::Refused)?;
                    Ok(Some(self.hold(handle, Some((Via::Server(String::from(IPD)), id)), false)))
                }
                How::Console => {
                    // A context's VM gets the console its relay serves; the relay holds the
                    // channel's.
                    if let Some(vm) = self.relay_vm.take() {
                        return Ok(Some(self.keep(vm)));
                    }
                    match (self.console.take(), self.consoled.clone()) {
                        // `sshd`'s channel console, told `ended` at the session's end.
                        (Some(handle), _) => Ok(Some(self.hold(handle, None, true))),
                        // The UART session's, minted at the steward's own console in the scope.
                        (None, Some(cons)) => {
                            let scope = Budget::from_handle(scope);
                            let (endpoint, id) = cons
                                .new_connection(&mut self.lend, "", 0, Some(&scope))
                                .map_err(|e| match e {
                                    redoubt_client::Error::Sys(Error::Timeout) => Error::Timeout,
                                    _ => Error::Refused,
                                })?;
                            Ok(Some(self.hold(endpoint.handle(), Some((Via::Console, id)), false)))
                        }
                        (None, None) => Ok(None),
                    }
                }
            }
        }

        fn console(&mut self, handle: Option<Handle>, relay: Option<Handle>) {
            for old in [
                core::mem::replace(&mut self.console, handle),
                core::mem::replace(&mut self.relay_console, relay),
            ]
            .into_iter()
            .flatten()
            {
                let _ = close(old);
            }
        }

        /// The steward's own copy of a connection minted in the owner's scope goes now: the
        /// scope's end frees its index for the next handle installed ([`Held`]), and the server
        /// is told at the loop's turn ([`Machine::clean`]). A copy the steward itself still uses
        /// (`sshd`'s channel console, a relay's control connection, a notices send right) is none
        /// of the scope's, and stays until its step is done.
        fn release(&mut self, key: u64) {
            if let Some(Held { handle, via: Some(_), .. }) = self.held.get_mut(&key) {
                if let Some(handle) = handle.take() {
                    let _ = close(handle);
                }
            }
        }

        /// Gives the connection held under `key` back to its server within [`CLEANUP_US`]:
        /// `ended` for `sshd`'s channel console (servers/steward.md, "Authentication and
        /// sessions"), else `disconnect` by its id through what minted it. A server that did not
        /// take the call in time is `Timeout`, and the record stays for the retry; on the
        /// attempt that parks the step a line says so, once (the step runs again when that
        /// server answers a call of the steward's, or goes with its restart). A connection at a
        /// server that restarted since has no record here ([`Machine::reconnected`]), and a
        /// server the steward no longer reaches nothing to tell: both are done. The cleanup
        /// after a scope's end, never the cutoff (R41).
        fn clean(&mut self, key: u64, parks: bool) -> Result<(), Error> {
            let Some(held) = self.held.get(&key) else { return Ok(()) };
            let (told, what) = match (held.console, held.handle, &held.via) {
                (true, Some(handle), _) => {
                    let ended = consol::Message::Ended(consol::Ended {})
                        .encode(&mut [])
                        .map_err(|_| Error::InvalidArgument)
                        .and_then(|m| {
                            Endpoint::from_handle(handle).send(&m, &[], None, CLEANUP_US).map_err(|(e, _)| e)
                        });
                    (ended, String::from("ended to sshd's channel"))
                }
                (_, _, Some((via, id))) => {
                    let (told, at) = match via {
                        Via::Server(name) => {
                            (self.unattached(name).ok().map(|n| disconnect(&n, *id)), name.as_str())
                        }
                        Via::Carved(k) => {
                            let kept = self.carved.get(k);
                            (
                                kept.map(|(h, _)| disconnect(&Nine::new(Endpoint::from_handle(*h)), *id)),
                                kept.map_or(k.as_str(), |(_, s)| s.as_str()),
                            )
                        }
                        Via::Console => {
                            let told = self.consoled.as_ref().map(|c| {
                                c.disconnect(*id, CLEANUP_US).map_err(|e| match e {
                                    redoubt_client::Error::Sys(e) => e,
                                    _ => Error::Refused,
                                })
                            });
                            (told, CONSOLED_NAME)
                        }
                    };
                    (told.unwrap_or(Ok(())), format!("disconnect {id} at {at}"))
                }
                _ => (Ok(()), String::new()),
            };
            if let Err(Error::Timeout) = told {
                if parks {
                    self.out.say(&format!(
                        "steward: {what} parked after {TRIES} attempts: retried when the server answers\n"
                    ));
                }
                return Err(Error::Timeout);
            }
            if let Some(Held { handle: Some(handle), .. }) = self.held.remove(&key) {
                let _ = close(handle);
            }
            Ok(())
        }

        /// By what minted the connection: the carve's server for one minted through a kept
        /// carve, `consoled` for the UART session's console, `sshd` for its channel console.
        fn server_of(&self, key: u64) -> Option<String> {
            let held = self.held.get(&key)?;
            if held.console {
                return Some(String::from(SSHD_NAME));
            }
            match &held.via.as_ref()?.0 {
                Via::Server(name) => Some(name.clone()),
                Via::Carved(k) => self.carved.get(k).map(|(_, s)| s.clone()),
                Via::Console => Some(String::from(CONSOLED_NAME)),
            }
        }

        /// The binding table's server for the slot, as [`Machine::connect`] mints; the console
        /// slot's is `consoled` only when no channel console waits to be handed over (a channel's
        /// is `sshd`'s, minted by nobody here); the vault's handle name for a crossing's.
        fn server_at(&self, owner: &Object, slot: Option<u16>) -> Option<String> {
            let principal = self.principal(&owner.domain)?;
            let labels = owner.domain.labels().as_slice();
            let Some(slot) = slot else {
                return self.own_lines.vault(&principal, labels).map(|v| v.handle.clone());
            };
            let bound = binding(
                &self.own_lines,
                &principal,
                labels,
                Child::of(owner.kind, labels),
                slot,
                self.agent_quota,
            )?;
            match bound.how {
                How::Console if self.console.is_none() && self.relay_vm.is_none() => {
                    Some(String::from(CONSOLED_NAME))
                }
                How::Console => None,
                how => how.server().map(String::from),
            }
        }

        /// The child is started with copies of the connections held under `connections`; the
        /// steward's own copies are closed once it runs, so that a scope's end leaves no stale
        /// index here ([`Held`]).
        fn launch(
            &mut self,
            owner: &Object,
            budget: Handle,
            connections: &[Option<u64>],
            context: Option<&str>,
        ) -> Result<(u64, Option<u64>), Error> {
            // The steward's badge and the shared slots, resolved on the stack: the launch is
            // the steward's heap peak, and this adds nothing to it.
            let keys = connections;
            let mut connections = [None; SLOTS as usize + 1];
            for (slot, key) in connections.iter_mut().zip(keys) {
                *slot = key.and_then(|k| self.handle_of(k));
            }
            let principal = self.principal(&owner.domain).ok_or(Error::BadHandle)?;
            let labels = owner.domain.labels().as_slice();
            let child = Child::of(owner.kind, labels);
            let bound: Vec<Option<Bound>> = (0..SLOTS)
                .map(|s| binding(&self.own_lines, &principal, labels, child, s, self.agent_quota))
                .collect();
            let exit = Endpoint::create()?;
            let exit_handle = exit.handle();
            // A session's notices endpoint (servers/steward.md, "Leases"): the VM is given the
            // receive right as the named handle `notices`, and the steward keeps the one send
            // right, closed with the session. An agent's VM is told of no lease's end.
            let notices = match child {
                Child::Session { .. } => {
                    let receive = Endpoint::create().inspect_err(|_| {
                        let _ = close(exit_handle);
                    })?;
                    let send = receive.mint(NonZeroU64::new(NOTICES).expect("NOTICES is not 0"), None);
                    match send {
                        Ok(send) => Some((receive, send)),
                        Err(e) => {
                            let _ = close(receive.handle());
                            let _ = close(exit_handle);
                            return Err(e);
                        }
                    }
                }
                Child::Lease { .. } => None,
            };
            // The VM's share, told to it as its budget's pages in its startup block, which it
            // sizes its limits by: a session's pages less its relay's; a lease's whole, it has
            // no relay. A lease's VM runs the agent's module, told the sponsor it acts for.
            let (vm_pages, told, module) = match child {
                Child::Lease { .. } => (self.agent_pages, lease_args(&principal), AGENT),
                Child::Session { .. } => (
                    self.session_pages.saturating_sub(RELAY_PAGES),
                    session_args(&self.own_lines, &principal, labels, context),
                    SHELL,
                ),
            };
            let endpoint = format!("endpoint={SYSTEM}");
            let len = self.program_len;
            let schedulers = format!("schedulers={SESSION_SCHEDULERS}");
            // The heap capped at what the VM's share holds beside each scheduler's stack and
            // thread page, its locks' endpoints and the process's own page (servers/init.md,
            // "Heaps").
            let threads = (SESSION_STACK_PAGES + 1) * SESSION_SCHEDULERS + SESSION_LOCK_PAGES;
            let heap = vm_pages.saturating_sub(threads as u64 + 1);
            // The steward's badge first, then the shared slots in order.
            let slot = |i: usize| connections.get(i + 1).copied().flatten();
            let started = {
                let mut read = |at: usize, buf: &mut [u8]| self.read_program(PROGRAM, at, buf);
                let mut launch =
                    Launch::streamed(STUB_BIN, len, &mut read, Budget::from_handle(budget), exit);
                if let Some(steward) = connections.first().copied().flatten() {
                    launch.handle("steward", steward);
                }
                launch.handle("budget", budget);
                if let Some((receive, _)) = &notices {
                    launch.handle(NOTICES_HANDLE, receive.handle());
                }
                for (i, b) in bound.iter().enumerate() {
                    let (Some(b), Some(conn)) = (b, slot(i)) else { continue };
                    if let Some(name) = b.name {
                        launch.handle(name, conn);
                    }
                    if let Some(at) = &b.at {
                        launch.namespace(at, conn);
                    }
                }
                launch
                    .stack_pages(SESSION_STACK_PAGES)
                    .heap_pages(u32::try_from(heap).unwrap_or(u32::MAX))
                    .budget_pages(vm_pages);
                told.iter().fold(launch.arg(&endpoint).arg(&schedulers), |l, arg| l.arg(arg)).arg(module);
                launch.start()
            };
            // The VM holds its copy of the receive right now, or the launch failed: the steward
            // keeps only the send right, and only for a VM that started.
            let send = notices.map(|(receive, send)| {
                let _ = close(receive.handle());
                send.handle()
            });
            let job = started.map_err(|failed| {
                let _ = close(exit_handle);
                if let Some(s) = send {
                    let _ = close(s);
                }
                match failed.error {
                    redoubt_client::Error::Sys(e) => e,
                    _ => Error::Refused,
                }
            })?;
            // The process ends with its budget; its notice comes to the exit endpoint.
            let _ = close(job.process().handle());
            // The child holds its copies now: the steward's go, but for a channel's console,
            // which `release` still tells `ended`.
            for key in keys.iter().flatten() {
                if let Some(held) = self.held.get_mut(key).filter(|h| !h.console) {
                    if let Some(handle) = held.handle.take() {
                        let _ = close(handle);
                    }
                }
            }
            if let Err(e) = self.watch_exit(exit_handle) {
                let _ = close(exit_handle);
                if let Some(s) = send {
                    let _ = close(s);
                }
                return Err(e);
            }
            Ok((u64::from(exit_handle.index()), send.map(|s| self.keep(s))))
        }

        /// The relay is launched from `/boot` in the context's budget with a hello badge of its
        /// own, and the launch waits for its hello: the VM's console, kept for the next console
        /// slot, and the control connection. Its exit is watched as a session's, so a relay that
        /// dies ends its context. The badge carries the steward's stamp, not the context's: the
        /// kernel stamps a mint only at or below the endpoint's own (kernel/abi.md, `mint`), and
        /// the relay's copy goes with the relay's process.
        fn launch_relay(&mut self, domain: &Domain, budget: Handle) -> Result<(u64, u64), Error> {
            self.hellos += 1;
            let badge = NonZeroU64::new(self.hellos).ok_or(Error::InvalidArgument)?;
            let hello = self.hello.as_ref().ok_or(Error::BadHandle)?.mint(badge, None)?;
            let exit = Endpoint::create()?;
            let exit_handle = exit.handle();
            let len = self.relay_len;
            let started = {
                let mut read = |at: usize, buf: &mut [u8]| self.read_program(RELAY, at, buf);
                let mut launch =
                    Launch::streamed(STUB_BIN, len, &mut read, Budget::from_handle(budget), exit);
                launch.handle(RELAY_HELLO, hello.handle());
                // Its share of the session's budget, the pages the VM is not told.
                launch.stack_pages(RELAY_STACK_PAGES).heap_pages(RELAY_HEAP_PAGES).budget_pages(RELAY_PAGES);
                launch.start()
            };
            let _ = close(hello.handle());
            let job = started.map_err(|failed| {
                let _ = close(exit_handle);
                match failed.error {
                    redoubt_client::Error::Sys(e) => e,
                    _ => Error::Refused,
                }
            })?;
            let _ = close(job.process().handle());
            if let Err(e) = self.watch_exit(exit_handle) {
                let _ = close(exit_handle);
                return Err(e);
            }
            // A relay that does not say hello is ended with its budget, which the failed step
            // destroys; its exit is watched already.
            let (vm, control) = self.hello(self.hellos)?;
            if let Some(old) = self.relay_vm.replace(vm) {
                let _ = close(old);
            }
            #[cfg(feature = "label-probe")]
            if !domain.labels().is_empty() && self.probe_victim.is_none() {
                self.probe_victim = Some(control);
            }
            #[cfg(not(feature = "label-probe"))]
            let _ = domain;
            let control = self.keep(control);
            Ok((u64::from(exit_handle.index()), control))
        }

        /// The login's channel console is kept, told `ended` when it is released, and the relay
        /// is given the channel's other connection, `sshd`'s for it, with `note`. Only the kept
        /// one can end the channel (servers/sshd.md, "A pty session").
        fn attach(&mut self, relay: u64, note: &str) -> Result<u64, Error> {
            let relay = self.handle_of(relay).ok_or(Error::BadHandle)?;
            let (Some(console), Some(theirs)) = (self.console.take(), self.relay_console.take()) else {
                self.console(None, None);
                return Err(Error::BadHandle);
            };
            let kept = self.hold(console, None, true);
            let m = consrelay::Message::Attach(consrelay::Attach { note });
            let told = typed::call_within::<consrelay::Protocol, _>(
                &Endpoint::from_handle(relay),
                &mut self.lend,
                &m,
                &[theirs],
                RELAY_US,
                |_, _| (),
            );
            // The relay holds its copy now, or none: the steward keeps only its own. A kept
            // console whose relay refused goes untold: `sshd`'s thread is in this login call,
            // so an `ended` could not be taken, and the refusal it gets ends the channel.
            let _ = close(theirs);
            if told.is_err() {
                if let Some(Held { handle: Some(handle), .. }) = self.held.remove(&kept) {
                    let _ = close(handle);
                }
                return Err(Error::Refused);
            }
            #[cfg(feature = "label-probe")]
            self.probe_cross(relay, console);
            Ok(kept)
        }

        /// The relay writes `note` to its channel and lets it go, within [`RELAY_US`]: past it,
        /// `Timeout`, which the core's driver counts as detached.
        fn detach(&mut self, relay: u64, note: &str) -> Result<(), Error> {
            let relay = self.handle_of(relay).ok_or(Error::BadHandle)?;
            let m = consrelay::Message::Detach(consrelay::Detach { note });
            typed::call_within::<consrelay::Protocol, _>(
                &Endpoint::from_handle(relay),
                &mut self.lend,
                &m,
                &[],
                RELAY_US,
                |_, _| (),
            )
            .map_err(|e| match e {
                redoubt_client::Error::Sys(Error::Timeout) => Error::Timeout,
                _ => Error::Refused,
            })
        }

        /// One page, one send, within [`NOTICE_US`]: a VM that does not take it in time misses
        /// the notice, and the page comes back and is unmapped with the error.
        fn notify(&mut self, notices: u64, lease: u64, name: &str) {
            let Some(notices) = self.handle_of(notices) else { return };
            let Ok(mut page) = Buffer::new(1) else { return };
            let m = steward::Message::LeaseEnded(steward::LeaseEnded { lease, name });
            let Ok(words) = m.encode(&mut page) else { return };
            let _ = Endpoint::from_handle(notices).send(&words, &[], Some(page), NOTICE_US);
        }

        fn notify_approval(&mut self, notices: u64) {
            let Some(notices) = self.handle_of(notices) else { return };
            let m = steward::Message::ApprovalWaiting(steward::ApprovalWaiting {});
            let Ok(words) = m.encode(&mut []) else { return };
            let _ = Endpoint::from_handle(notices).send(&words, &[], None, NOTICE_US);
        }

        fn approval_consoles(&mut self, console: Option<Handle>, reader: Option<Handle>) {
            let next = match (console, reader) {
                (Some(c), Some(r)) => Some((c, r)),
                (c, r) => {
                    for h in [c, r].into_iter().flatten() {
                        let _ = close(h);
                    }
                    None
                }
            };
            if let Some((c, r)) = core::mem::replace(&mut self.approval, next) {
                let _ = close(c);
                let _ = close(r);
            }
        }

        /// The channel is handed to a reader: its console and a send right of the steward's own
        /// badge `APPROVAL`, on the readers' work endpoint; an idle reader takes it, and a new one
        /// starts only when every reader is busy (`Watchers`, as the exit watchers are pooled).
        /// Past [`CHANNELS`] open channels the open is refused and the consoles closed.
        fn channel_opened(&mut self, channel: u64) -> Result<(), Error> {
            let (console, reader) = self.approval.take().ok_or(Error::BadHandle)?;
            // Each stage named, so a channel the steward cannot serve says which call refused.
            let started = (|| {
                if self.channels.len() >= CHANNELS {
                    return Err(("the channel cap", Error::Refused));
                }
                let badge = NonZeroU64::new(APPROVAL).expect("APPROVAL is not 0");
                let send = self.own.mint(badge, None).map_err(|e| ("the badge", e))?;
                let new = self.readers.watch();
                let thread = if new {
                    Buffer::new(READER_STACK_PAGES).map_err(|e| ("the stack", e)).and_then(|stack| {
                        let work = self.reader_work.handle().index() as usize;
                        redoubt_rt::handle::thread_create(reader_thread, stack, work)
                            .map(|_| ())
                            .map_err(|e| ("the thread", e))
                    })
                } else {
                    Ok(())
                };
                let handed = thread.and_then(|()| {
                    self.reader_hand
                        .send(
                            &[channel & 0xffff_ffff, channel >> 32, 0, 0],
                            &[reader, send.handle()],
                            None,
                            HAND_US,
                        )
                        .map_err(|(e, _)| {
                            // The thread, if it started, waits idle in the pool.
                            self.readers.unwatch(false);
                            ("the hand-over", e)
                        })
                });
                if matches!(handed, Err(("the stack", _)) | Err(("the thread", _))) {
                    self.readers.unwatch(new);
                }
                let _ = close(send.handle());
                handed
            })();
            match started {
                Ok(()) => {
                    let _ = close(reader);
                    self.channels.insert(channel, console);
                    Ok(())
                }
                Err((what, e)) => {
                    let _ = close(console);
                    let _ = close(reader);
                    self.out.say(&format!(
                        "steward: approval channel {channel:016x} not served: {what} ({e:?})\n"
                    ));
                    Err(e)
                }
            }
        }

        /// The new connection stands in for the startup block's handle (a second restart's for
        /// the first's, closed). What was minted at the old instance is dead there: the attached
        /// connection, the carves kept at it and every child's connection, held by ids the new
        /// instance never drew; their records go, and nothing is disconnected.
        fn reconnected(&mut self, server: &str, connection: Handle) {
            if let Some(old) = self.replaced.insert(server.into(), connection) {
                let _ = close(old);
            }
            self.servers.remove(server);
            let carved_there: Vec<String> =
                self.carved.iter().filter(|(_, (_, s))| s == server).map(|(k, _)| k.clone()).collect();
            for key in &carved_there {
                self.homes.remove(key);
                if let Some((handle, _)) = self.carved.remove(key) {
                    let _ = close(handle);
                }
            }
            let dead = |held: &Held| match &held.via {
                Some((Via::Server(s), _)) => s == server,
                Some((Via::Carved(k), _)) => carved_there.contains(k),
                _ => false,
            };
            let gone: Vec<u64> = self.held.iter().filter(|(_, h)| dead(h)).map(|(k, _)| *k).collect();
            for key in gone {
                if let Some(Held { handle: Some(handle), .. }) = self.held.remove(&key) {
                    let _ = close(handle);
                }
            }
        }

        /// A screen that cannot be written within the bound is dropped and said once, with why:
        /// the person asks again with `pending`.
        fn screen(&mut self, channel: u64, text: &str) {
            let Some(console) = self.channels.get(&channel) else { return };
            let endpoint = Endpoint::from_handle(*console);
            let written = Connection::attach_within(endpoint, &mut self.lend, SCREEN_US).and_then(|conn| {
                let file = conn.open(&mut self.lend, "", mode::OWRITE)?;
                let n = file.write_at(&mut self.lend, 0, text.as_bytes())?;
                let _ = file.close(&mut self.lend);
                Ok(n)
            });
            if let Err(e) = written {
                self.out
                    .say(&format!("steward: a screen on channel {channel:016x} was not written: {e:?}\n"));
            }
        }

        fn channel_closed(&mut self, channel: u64) {
            if let Some(console) = self.channels.remove(&channel) {
                let _ = close(console);
            }
        }

        fn slots_at(&self, owner: &Object, server: &str) -> Vec<u16> {
            let Some(principal) = self.principal(&owner.domain) else { return Vec::new() };
            let labels = owner.domain.labels().as_slice();
            let child = Child::of(owner.kind, labels);
            (0..SLOTS)
                .filter(|&slot| {
                    binding(&self.own_lines, &principal, labels, child, slot, self.agent_quota)
                        .is_some_and(|b| b.at.is_some() && b.how.server() == Some(server))
                })
                .collect()
        }

        /// As [`Machine::notify`]: one page, one send, within [`NOTICE_US`].
        fn restarted(&mut self, notices: u64, server: &str, slot: u16) {
            let Some(notices) = self.handle_of(notices) else { return };
            let Ok(mut page) = Buffer::new(1) else { return };
            let m = steward::Message::ServerRestarted(steward::ServerRestarted { server, slot });
            let Ok(words) = m.encode(&mut page) else { return };
            let _ = Endpoint::from_handle(notices).send(&words, &[], Some(page), NOTICE_US);
        }

        /// The slot's binding again, as [`Machine::connect`] makes it, for a slot at a shared
        /// server bound at a path; the steward's copy of the handle goes with the reply.
        fn rebind(
            &mut self,
            owner: &Object,
            slot: u16,
            scope: Handle,
        ) -> Result<Option<(u64, Handle, String)>, Error> {
            let principal = self.principal(&owner.domain).ok_or(Error::BadHandle)?;
            let labels = owner.domain.labels().as_slice();
            let child = Child::of(owner.kind, labels);
            let Some(bound) = binding(&self.own_lines, &principal, labels, child, slot, self.agent_quota)
            else {
                return Ok(None);
            };
            let (Some(at), Some(_)) = (bound.at, bound.how.server()) else { return Ok(None) };
            let Some(key) = self.connect(owner, slot, scope)? else { return Ok(None) };
            let handle = self.held.get_mut(&key).and_then(|h| h.handle.take()).ok_or(Error::BadHandle)?;
            Ok(Some((key, handle, at)))
        }

        /// The crossing program (servers/steward.md, "Declassification and push"; R42): one
        /// connection to the domain's vault at its root (a mint below the root walks the path
        /// with the caller's labels, R25, which the unlabelled steward has not: the labelled
        /// program walks to the item), minted in `budget` as its scope so the budget's
        /// destruction ends it in every table (R41), and an endpoint the steward makes and keeps
        /// a call right on, whose receive right the program is given, so the labelled program
        /// only ever answers (R1). Launched from `/boot` through the stub, as a relay is, and
        /// called once within `CROSSING_LIFE`, the budget's own deadline. The steward's copies of
        /// what the program holds go once it runs; the connection is held under a key for the
        /// disconnect after the destroy.
        fn cross(
            &mut self,
            owner: &Object,
            budget: Handle,
            path: &str,
            bytes: Option<&[u8]>,
        ) -> Result<(u64, Option<u64>, Vec<u8>), Error> {
            let principal = self.principal(&owner.domain).ok_or(Error::BadHandle)?;
            let labels = owner.domain.labels().as_slice();
            let vault = self.own_lines.vault(&principal, labels).cloned().ok_or(Error::BadHandle)?;
            let key = self.fresh(&vault.handle, "", budget)?.ok_or(Error::Refused)?;
            let result = self.cross_through(key, budget, path, bytes);
            if result.is_err() {
                self.release(key);
            }
            result.map(|(pid, bytes)| (pid, Some(key), bytes))
        }

        fn home_stat(&mut self, principal: &str, path: &str) -> Result<Option<u64>, Error> {
            let home = self.home(principal)?;
            match home.stat(&mut self.lend, path) {
                Ok(stat) => Ok(Some(stat.length)),
                Err(redoubt_client::Error::Rerror(_)) => Ok(None),
                Err(_) => Err(Error::Refused),
            }
        }

        fn home_read(&mut self, principal: &str, path: &str, max: usize) -> Result<Vec<u8>, Error> {
            let home = self.home(principal)?;
            let file = home.open(&mut self.lend, path, mode::OREAD).map_err(|_| Error::Refused)?;
            let mut out = Vec::new();
            out.try_reserve(max + 1).map_err(|_| Error::OutOfMemory)?;
            out.resize(max + 1, 0);
            let mut done = 0;
            let mut read = Ok(());
            while done < out.len() {
                match file.read_at(&mut self.lend, done as u64, &mut out[done..]) {
                    Ok(0) => break,
                    Ok(n) => done += n,
                    Err(_) => {
                        read = Err(Error::Refused);
                        break;
                    }
                }
            }
            let _ = file.close(&mut self.lend);
            read?;
            if done > max {
                return Err(Error::TooLarge);
            }
            out.truncate(done);
            Ok(out)
        }

        /// `path`'s directory is made if it is missing; a file at `path` already there refuses
        /// the write, so nothing is ever overwritten by the steward.
        fn home_write(&mut self, principal: &str, path: &str, bytes: &[u8]) -> Result<(), Error> {
            let home = self.home(principal)?;
            if home.stat(&mut self.lend, path).is_ok() {
                return Err(Error::Busy);
            }
            let (dir, name) = redoubt_steward_server::drive::split(path);
            if !dir.is_empty() && home.stat(&mut self.lend, dir).is_err() {
                let (parent, leaf) = redoubt_steward_server::drive::split(dir);
                let made = home
                    .create(&mut self.lend, parent, leaf, DMDIR | 0o755, mode::OREAD)
                    .map_err(|_| Error::Refused)?;
                let _ = made.close(&mut self.lend);
            }
            let file =
                home.create(&mut self.lend, dir, name, 0o644, mode::OWRITE).map_err(|_| Error::Refused)?;
            let mut done = 0;
            let mut wrote = Ok(());
            while done < bytes.len() {
                match file.write_at(&mut self.lend, done as u64, &bytes[done..]) {
                    Ok(0) | Err(_) => {
                        wrote = Err(Error::Refused);
                        break;
                    }
                    Ok(n) => done += n,
                }
            }
            let _ = file.close(&mut self.lend);
            wrote
        }

        fn random(&mut self) -> Result<u64, Error> { redoubt_rt::handle::random_u64() }

        fn now(&mut self) -> u64 { redoubt_rt::handle::time_now().unwrap_or(0) }
    }

    /// A mint's failure as the kernel's error: a server that did not take the call within the
    /// client's bound is `Timeout` and has answered nothing (a step parked there stays parked);
    /// one that refused, or answered nothing readable, `Refused`.
    fn minted(e: redoubt_rt::client::ClientError) -> Error {
        match e {
            redoubt_rt::client::ClientError::Sys(Error::Timeout) => Error::Timeout,
            _ => Error::Refused,
        }
    }

    /// `disconnect` by `id` through `nine`, within [`CLEANUP_US`], as the kernel's error: a server
    /// that did not take the call is `Timeout`; one that refused, or answered nothing readable,
    /// `Refused`.
    fn disconnect(nine: &Nine, id: u64) -> Result<(), Error> {
        nine.disconnect(id, CLEANUP_US).map_err(|e| match e {
            redoubt_rt::client::ClientError::Sys(e) => e,
            _ => Error::Refused,
        })
    }

    impl Machine<'_> {
        /// The launch and the one call of [`Machine::cross`], once the connection is held under
        /// `key`: the process id and the bytes a read answered.
        fn cross_through(
            &mut self,
            key: u64,
            budget: Handle,
            path: &str,
            bytes: Option<&[u8]>,
        ) -> Result<(u64, Vec<u8>), Error> {
            let volume = self.handle_of(key).ok_or(Error::BadHandle)?;
            let serve = Endpoint::create()?;
            let call = match serve.mint(NonZeroU64::new(CROSSING_CALL).expect("CROSSING_CALL is not 0"), None)
            {
                Ok(call) => call,
                Err(e) => {
                    let _ = close(serve.handle());
                    return Err(e);
                }
            };
            let exit = match Endpoint::create() {
                Ok(exit) => exit,
                Err(e) => {
                    let _ = close(serve.handle());
                    let _ = close(call.handle());
                    return Err(e);
                }
            };
            let exit_handle = exit.handle();
            let len = self.crossing_len;
            let pages = self.crossing_pages;
            let started = {
                let mut read = |at: usize, buf: &mut [u8]| self.read_program(CROSSING, at, buf);
                let mut launch =
                    Launch::streamed(STUB_BIN, len, &mut read, Budget::from_handle(budget), exit);
                launch.handle(CROSSING_SERVE, serve.handle());
                launch.handle(CROSSING_VOLUME, volume);
                launch.stack_pages(CROSSING_STACK_PAGES).heap_pages(CROSSING_HEAP_PAGES).budget_pages(pages);
                launch.arg(if bytes.is_some() { "write" } else { "read" });
                launch.start()
            };
            // The program holds its copies of the endpoint and the connection now, or none.
            let _ = close(serve.handle());
            if let Some(held) = self.held.get_mut(&key) {
                if let Some(handle) = held.handle.take() {
                    let _ = close(handle);
                }
            }
            let job = started.map_err(|failed| {
                let _ = close(exit_handle);
                let _ = close(call.handle());
                match failed.error {
                    redoubt_client::Error::Sys(e) => e,
                    _ => Error::Refused,
                }
            })?;
            let _ = close(job.process().handle());
            if let Err(e) = self.watch_exit(exit_handle) {
                let _ = close(exit_handle);
                let _ = close(call.handle());
                return Err(e);
            }
            let pid = u64::from(exit_handle.index());
            let answered = match bytes {
                None => typed::call_within::<crossing::Protocol, _>(
                    &call,
                    &mut self.lend,
                    &crossing::Message::Read(crossing::Read { path }),
                    &[],
                    CROSSING_LIFE,
                    |r, _| match r {
                        crossing::Reply::Read(r) => Some(r.bytes.to_vec()),
                        _ => None,
                    },
                ),
                Some(bytes) => typed::call_within::<crossing::Protocol, _>(
                    &call,
                    &mut self.lend,
                    &crossing::Message::Write(crossing::Write { path, bytes }),
                    &[],
                    CROSSING_LIFE,
                    |r, _| match r {
                        crossing::Reply::Write(_) => Some(Vec::new()),
                        _ => None,
                    },
                ),
            };
            let _ = close(call.handle());
            match answered {
                Ok(Some(bytes)) => Ok((pid, bytes)),
                Ok(None) => Err(Error::Refused),
                Err(redoubt_client::Error::Server(_)) => Err(Error::Refused),
                Err(redoubt_client::Error::Sys(e)) => Err(e),
                Err(_) => Err(Error::Refused),
            }
        }
    }

    /// The steward's console (servers/steward.md, "Failure and restart"): the lines it says,
    /// queued in order, and the fid they are written through, kept open on the connection `init`
    /// gave it at `/dev/cons` with its one lent page. Not the runtime's `start::say`, which
    /// attaches and clunks per line: the connection is attached once, here, and the UART
    /// session's console is minted through it ([`Machine::consoled`]), so no second `Tversion`
    /// ends the kept fid.
    struct Console {
        lines: Queue,
        /// None when `init` gave the steward no console: its lines go nowhere.
        at: Option<Out>,
    }

    /// The kept connection's side of the queue: its calls bounded by [`CONSOLE_US`] through a
    /// second client on the same connection (the attached one's calls are not bounded: a mint).
    struct Out {
        conn: Connection,
        nine: Nine,
        lend: Lend,
        fid: Option<u32>,
    }

    impl Console {
        fn new(startup: &Startup) -> Console {
            let at = startup.namespace().find(|(path, _)| *path == "/dev/cons").and_then(|(_, handle)| {
                let mut lend = Lend::new(1).ok()?;
                let conn = Connection::attach(Endpoint::from_handle(handle), &mut lend).ok()?;
                let nine = Nine::within(Endpoint::from_handle(handle), CONSOLE_US);
                Some(Out { conn, nine, lend, fid: None })
            });
            Console { lines: Queue::default(), at }
        }

        /// The attached connection, through which the UART session's console is minted.
        fn connection(&self) -> Option<Connection> { self.at.as_ref().map(|o| o.conn.clone()) }

        /// Queues `line`, written at the next turn.
        fn say(&mut self, line: &str) {
            if self.at.is_some() {
                self.lines.push(String::from(line));
            }
        }

        /// A turn at `now` (`redoubt_steward_server::console::Queue::run`): when the next may run.
        fn turn(&mut self, now: u64) -> Option<u64> {
            let out = self.at.as_mut()?;
            self.lines.run(now, out)
        }

        /// The last turn, at the steward's exit: every line queued, up to the first the console
        /// did not take.
        fn flush(&mut self) {
            if let Some(out) = self.at.as_mut() {
                self.lines.flush(out);
            }
        }
    }

    impl Sink for Out {
        fn open(&mut self) -> Result<(), Error> {
            // A fid the console refused a write on is gone there (a restarted `consoled` knows
            // none of its clients' fids) and goes back to the pool here; one that timed out may
            // still be the server's, and stays taken, as the client library keeps such a fid.
            let fid = self.conn.take_fid().map_err(|_| Error::OutOfMemory)?;
            let opened = self
                .nine
                .walk(&mut self.lend, ROOT, fid, "")
                .and_then(|_| self.nine.open(&mut self.lend, fid, mode::OWRITE));
            match opened {
                Ok(_) => {
                    if let Some(old) = self.fid.replace(fid) {
                        self.conn.free_fid(old);
                    }
                    Ok(())
                }
                Err(e) => {
                    if !matches!(e, ClientError::Sys(Error::Timeout)) {
                        self.conn.free_fid(fid);
                    }
                    Err(sys(e))
                }
            }
        }

        fn write(&mut self, bytes: &[u8]) -> Result<usize, Error> {
            let fid = self.fid.ok_or(Error::BadHandle)?;
            self.nine.write(&mut self.lend, fid, 0, bytes).map_err(sys)
        }
    }

    /// A client error as the queue reads it: the call's own error (a `Timeout` keeps the line),
    /// anything else a refusal.
    fn sys(e: ClientError) -> Error {
        match e {
            ClientError::Sys(e) | ClientError::Pages(e) => e,
            _ => Error::Refused,
        }
    }

    fn labels(l: &[u64]) -> String {
        let items: Vec<String> = l.iter().map(|x| format!("{x}")).collect();
        format!("{{{}}}", items.join(","))
    }

    /// Audit records are console lines until the audit file (M3); so is a console session the
    /// steward could not open.
    fn deliver(out: &mut Console, outputs: Vec<Output>) {
        for o in outputs {
            if let Output::Reply { answer, .. } = &o {
                out.say(&format!("steward: the console session did not open: {answer:?}\n"));
            }
            if let Output::Audit(a) = o {
                let d = a.domain();
                let line = format!(
                    "steward: audit {}/{} at {}: {:?}\n",
                    d.account(),
                    labels(d.labels().as_slice()),
                    a.at(),
                    a.record()
                );
                out.say(&line);
            }
        }
    }

    /// Says each batch step that failed, with the kernel's error: the core's answer names only
    /// `Failed`.
    fn failed(out: &mut Console, steward: &mut Steward<Handle, u64>) {
        for (step, error) in core::mem::take(&mut steward.failed) {
            out.say(&format!("steward: a step failed ({error:?}): {step:?}\n"));
        }
    }

    /// What an event did, said: its failed steps, its outputs, and the sub-budgets that changed;
    /// with no outputs, false: the core exited.
    fn after(
        out: &mut Console,
        steward: &mut Steward<Handle, u64>,
        usage: &mut BTreeMap<u32, (u64, u64)>,
        outputs: Option<Vec<Output>>,
    ) -> bool {
        let Some(outputs) = outputs else { return false };
        failed(out, steward);
        deliver(out, outputs);
        tree(out, steward, usage);
        true
    }

    /// The operation of a call whose time the steward says, by its opcode: `end_lease` and
    /// `submit` (libs/wire/tables/steward.md).
    fn served_op(words: &redoubt_rt::ipc::Words) -> Option<&'static str> {
        match words[0] {
            9 => Some("submit"),
            11 => Some("end_lease"),
            _ => None,
        }
    }

    /// The core exited: said once, and the steward's exit code.
    fn core_exited(out: &mut Console) -> u32 {
        out.say("steward: the core exited\n");
        CORE_EXITED
    }

    /// What each domain's sub-budget holds, said when it changes: the kernel's count of the
    /// pages and processes carved from it, which a session's budget takes and its end gives back.
    fn tree(out: &mut Console, steward: &Steward<Handle, u64>, last: &mut BTreeMap<u32, (u64, u64)>) {
        for c in &steward.carved {
            for (d, sub) in &c.subs {
                let Ok(u) = Budget::from_handle(*sub).usage() else { continue };
                let now = (u.pages_usage, u64::from(u.processes_usage));
                if last.insert(sub.index(), now).is_some_and(|was| was != now) {
                    let line = format!(
                        "steward: users/{}/{} holds {} pages, {} processes\n",
                        c.name,
                        labels(d.labels().as_slice()),
                        now.0,
                        now.1
                    );
                    out.say(&line);
                }
            }
        }
    }

    /// `latency-probe` only: the probe's state (see [`LATENCY_LEASE_US`]).
    #[cfg(feature = "latency-probe")]
    #[derive(Default)]
    struct Probe {
        /// The timeout armed, and which: `false` the timer's, `true` the decision's.
        due: Option<(u64, bool)>,
        /// The timer woke: the decision's timeout is armed at the last point before the receive
        /// that waits on it, as the timer's is, so its wake measures the kernel's expiry to the
        /// steward running and not the steward's own lines written before it waits.
        decide: bool,
        /// A probe lease whose `start_agent` was just answered, with the answer's time: the
        /// timer is armed at the last point before the receive that waits on it, so the wake
        /// measures the kernel's expiry to the steward running, not the steward's own work
        /// after the reply (its audit and usage lines through consoled), which is the
        /// `reply_wait` sample instead.
        pending: Option<(u64, u64, u64)>,
        /// The probe lease and the badge of the session that started it.
        lease: u64,
        badge: u64,
        /// Samples taken since the last flush, by measure and group.
        counts: BTreeMap<(&'static str, usize), u64>,
        /// Flushes so far: the case's rounds ([`LATENCY_ROUNDS`]).
        rounds: usize,
    }

    #[cfg(feature = "latency-probe")]
    impl Probe {
        /// One sample, printed as the stand-in prints it (tests/programs/src/sched.rs), under
        /// the steward's console prefix the oracle cuts.
        fn sample(&mut self, out: &mut Console, measure: &'static str, n: usize, end: u64, gross: u64) {
            *self.counts.entry((measure, n)).or_default() += 1;
            out.say(&format!("steward: LATENCY-SAMPLE N={n} {measure} {end} {gross}\n"));
        }

        /// The counts, once the domain's leases are all gone: one line per measure and group.
        /// After the last round's, the window's end, `LATENCY-END` with the steward's clock: the
        /// oracle judges no destruction ending after it and no sample printed after it (the
        /// marker-file exits that follow reap every lease's VM, none of them a sample).
        fn flush(&mut self, out: &mut Console) {
            for ((measure, n), count) in core::mem::take(&mut self.counts) {
                out.say(&format!("steward: LATENCY-COUNT N={n} {measure} {count}\n"));
            }
            self.rounds += 1;
            if self.rounds == LATENCY_ROUNDS {
                let now = redoubt_rt::handle::time_now().unwrap_or(0);
                out.say(&format!("steward: LATENCY-END {now}\n"));
            }
        }
    }

    /// `latency-probe` only: the live leases, `lease` itself among them (the case's N: the
    /// spinners beside the probe's or the deadline's lease), the sample's group; a lease the core
    /// has already let go of counts itself in.
    #[cfg(feature = "latency-probe")]
    fn others(steward: &Steward<Handle, u64>, lease: u64) -> usize {
        let index = inspect::index(&steward.store);
        let live = leases_live(steward);
        if index.ids.contains_key(&lease) { live } else { live + 1 }
    }

    #[cfg(feature = "latency-probe")]
    fn leases_live(steward: &Steward<Handle, u64>) -> usize {
        inspect::domains(&steward.store).map(|(_, d)| d.leases.len()).sum()
    }

    /// `latency-probe` only: the probe lease a `start_agent` from `badge` just started, if the
    /// newest lease of its domain asked for exactly [`LATENCY_LEASE_US`].
    #[cfg(feature = "latency-probe")]
    fn probe_lease(steward: &Steward<Handle, u64>, badge: u64) -> Option<u64> {
        let index = inspect::index(&steward.store);
        let route = index.routes.get(&badge)?;
        let state = inspect::domain(&steward.store, &route.domain)?;
        let (id, lease) = state.leases.iter().max_by_key(|(_, l)| l.number)?;
        (lease.lease == LATENCY_LEASE_US).then_some(*id)
    }

    /// Serves until the endpoint is destroyed; the lines left unsaid at the exit go last.
    pub fn serve(startup: &Startup) -> u32 {
        let mut out = Console::new(startup);
        let code = run(startup, &mut out);
        out.flush();
        code
    }

    fn run(startup: &Startup, out: &mut Console) -> u32 {
        let (Some(users), Some(endpoint)) = (startup.handle("users"), startup.handle("steward")) else {
            return NO_HANDLE;
        };
        let own = Endpoint::from_handle(endpoint);
        let Ok(lend) = Lend::new(LEND_PAGES) else { return NOT_STARTED };
        let args: Vec<&str> = startup.args().collect();
        let lines: Vec<&str> = own_args(&args).collect();
        let work = Endpoint::create();
        let Some((work, work_send)) = work
            .ok()
            .and_then(|w| w.mint(NonZeroU64::new(WORK).expect("WORK is not 0"), None).ok().map(|s| (w, s)))
        else {
            out.say("steward: cannot make its watchers' work endpoint\n");
            return NOT_STARTED;
        };
        let Some((reader_work, reader_hand)) = Endpoint::create()
            .ok()
            .and_then(|w| w.mint(NonZeroU64::new(HAND).expect("HAND is not 0"), None).ok().map(|s| (w, s)))
        else {
            out.say("steward: cannot make its readers' work endpoint\n");
            return NOT_STARTED;
        };
        let consoled = out.connection();
        let mut machine = Machine {
            startup,
            out,
            lend,
            own: Endpoint::from_handle(endpoint),
            servers: BTreeMap::new(),
            consoled,
            console: None,
            relay_console: None,
            held: BTreeMap::new(),
            next_key: 0,
            carved: BTreeMap::new(),
            homes: BTreeMap::new(),
            replaced: BTreeMap::new(),
            accounts: Vec::new(),
            own_lines: Own::default(),
            session_pages: 0,
            agent_pages: 0,
            agent_quota: 0,
            program_len: 0,
            relay_len: 0,
            crossing_len: 0,
            crossing_pages: 0,
            hello: Endpoint::create().ok(),
            hellos: 0,
            relay_vm: None,
            #[cfg(feature = "label-probe")]
            probe_victim: None,
            #[cfg(feature = "label-probe")]
            probe_due: None,
            work,
            work_send,
            watchers: Watchers::default(),
            approval: None,
            channels: BTreeMap::new(),
            reader_work,
            reader_hand,
            readers: Watchers::default(),
            #[cfg(feature = "latency-probe")]
            last_destroy: None,
        };
        // The kernel's count of what `users` holds as this instance starts: 0, at boot and after
        // `init` emptied a dead steward's carves. The start's lines go as they are said, not at
        // the loop's first turn: a steward that dies in its console session's launch
        // (`steward-restart-reboot`) has said what `users` held and what it carved.
        if let Ok(u) = Budget::from_handle(users).usage() {
            machine.out.say(&format!(
                "steward: users holds {} pages, {} processes\n",
                u.pages_usage, u.processes_usage
            ));
            machine.out.turn(0);
        }
        let mut steward: Steward<Handle, u64> = match start(&lines, users, &mut machine) {
            Ok(s) => s,
            Err(e) => {
                machine.out.say(&format!("{e}\n"));
                return NOT_STARTED;
            }
        };
        for c in &steward.carved {
            let subs: Vec<String> = c.subs.iter().map(|(d, _)| labels(d.labels().as_slice())).collect();
            machine.out.say(&format!(
                "steward: carved users/{} account {}: {}\n",
                c.name,
                c.account,
                subs.join(" ")
            ));
        }
        machine.out.turn(0);
        // Every principal's account and name, a sponsored principal's too: its sessions are
        // carved under its sponsor's, but their namespace and arguments are its own.
        machine.accounts = inspect::fixed(&steward.store)
            .principals
            .iter()
            .map(|p| (p.account.get(), p.name.clone()))
            .collect();
        machine.own_lines = steward.own.clone();
        machine.session_pages = steward.sizes.session.pages;
        machine.agent_pages = steward.sizes.agent.pages;
        machine.agent_quota = steward.sizes.agent_quota;
        machine.crossing_pages = steward.sizes.crossing.pages;
        // A box with no program for its sessions starts them all failed: the steward still runs.
        if let Ok(conn) = machine.server("bootfsd") {
            if let Ok(stat) = conn.stat(&mut machine.lend, PROGRAM) {
                machine.program_len = stat.length as usize;
            }
            if let Ok(stat) = conn.stat(&mut machine.lend, RELAY) {
                machine.relay_len = stat.length as usize;
            }
            if let Ok(stat) = conn.stat(&mut machine.lend, CROSSING) {
                machine.crossing_len = stat.length as usize;
            }
        }
        // The case is over: this instance and the five after it exit at once, and init reboots.
        #[cfg(feature = "latency-probe")]
        if let Some(console) = steward.own.console.clone() {
            if matches!(machine.home_stat(&console, LATENCY_DONE), Ok(Some(_))) {
                machine.out.say("steward: latency-probe: the case is done, exiting for the reboot\n");
                return LATENCY_EXIT;
            }
        }
        let Ok(report) = own.mint(NonZeroU64::new(EXITS).expect("EXITS is not 0"), None) else {
            machine.out.say("steward: cannot mint its exit reports' badge\n");
            return NOT_STARTED;
        };
        REPORT.store(report.handle().index(), Ordering::Release);
        let Ok(done) = own.mint(NonZeroU64::new(READER_DONE).expect("READER_DONE is not 0"), None) else {
            machine.out.say("steward: cannot mint its readers' badge\n");
            return NOT_STARTED;
        };
        READER_REPORT.store(done.handle().index(), Ordering::Release);
        let mut usage = BTreeMap::new();
        tree(machine.out, &steward, &mut usage);
        let outputs = steward.open_console(&mut machine).ok();
        if !after(machine.out, &mut steward, &mut usage, outputs) {
            return core_exited(machine.out);
        }
        #[cfg(feature = "restart-probe")]
        let probe_at = redoubt_rt::handle::time_now().unwrap_or(0).saturating_add(PROBE_DELAY);
        // `sshd`'s `watch` calls, held unanswered while this instance runs: its end fails each
        // with `Dead` (R4b), and so tells `sshd` its sessions are over.
        let mut watches: Vec<Request> = Vec::new();
        #[cfg(feature = "latency-probe")]
        let mut probe = Probe::default();
        loop {
            // The idle timer: a detached context past its principal's bound ends (servers/steward.md,
            // "Contexts"); the receive below waits no longer than the next one's bound.
            let now = redoubt_rt::handle::time_now().unwrap_or(0);
            if steward.store.next_idle().is_some_and(|t| t <= now) {
                let outputs = steward.event(&mut machine, EventKind::Idle, 0).ok();
                if !after(machine.out, &mut steward, &mut usage, outputs) {
                    return core_exited(machine.out);
                }
            }
            let idle = steward.store.next_idle().map(|t| t.saturating_sub(now).max(1));
            // The cleanup steps due: each connection an end gave back since the last turn, and
            // the retries whose time has come, each within CLEANUP_US, up to the first that
            // times out; the receive below wakes for the next retry, or at once for the steps
            // a timeout left due. Then the console lines queued since the last turn (the
            // steps' own among them), each within CONSOLE_US, up to the first the console did
            // not take; the receive wakes for that line's retry.
            let due = steward.cleanup(&mut machine).map(|t| t.saturating_sub(now).max(1));
            let line_due = machine.out.turn(now).map(|t| t.saturating_sub(now).max(1));
            #[cfg(feature = "restart-probe")]
            let wait = probe_at.saturating_sub(redoubt_rt::handle::time_now().unwrap_or(probe_at));
            #[cfg(not(feature = "restart-probe"))]
            let wait = FOREVER;
            #[cfg(feature = "label-probe")]
            let wait = match machine.probe_due {
                Some((due, _)) => {
                    wait.min(due.saturating_sub(redoubt_rt::handle::time_now().unwrap_or(due)).max(1))
                }
                None => wait,
            };
            let wait = [idle, due, line_due].into_iter().flatten().fold(wait, u64::min);
            #[cfg(feature = "latency-probe")]
            let wait = match probe.due {
                Some((at, _)) => {
                    wait.min(at.saturating_sub(redoubt_rt::handle::time_now().unwrap_or(at)).max(1))
                }
                None => wait,
            };
            // The probe's samples are counted out when the round's leases are gone and the case
            // has marked the round's end (at N = 1 no spinner outlives a probe lease, so the
            // leases are gone after each one: the marker, not the count, closes a round); while
            // the counts wait for it, the receive below looks again within half a second. After
            // the case's rounds, its marker file ends this instance for the reboot.
            #[cfg(feature = "latency-probe")]
            let wait = if !probe.counts.is_empty() && leases_live(&steward) == 0 {
                let marker = format!("{LATENCY_ROUND}{}", probe.rounds + 1);
                let marked = steward
                    .own
                    .console
                    .clone()
                    .is_some_and(|console| matches!(machine.home_stat(&console, &marker), Ok(Some(_))));
                if marked {
                    probe.flush(machine.out);
                    // The counts go now, not at the next event's turn.
                    machine.out.turn(now);
                    wait
                } else {
                    wait.min(LATENCY_POLL_US)
                }
            } else {
                wait
            };
            #[cfg(feature = "latency-probe")]
            if probe.rounds >= LATENCY_ROUNDS && probe.due.is_none() {
                if let Some(console) = steward.own.console.clone() {
                    if matches!(machine.home_stat(&console, LATENCY_DONE), Ok(Some(_))) {
                        machine.out.say("steward: latency-probe: the case is done, exiting for the reboot\n");
                        return LATENCY_EXIT;
                    }
                }
            }
            // The probe's timer, armed here and nowhere earlier: the steward is about to wait.
            // The sample's own line waits for the next turn, the timer's wake.
            #[cfg(feature = "latency-probe")]
            let wait = match probe.pending.take() {
                Some((lease, badge, replied)) => {
                    let now = redoubt_rt::handle::time_now().unwrap_or(replied);
                    let n = others(&steward, lease);
                    probe.sample(machine.out, "reply_wait", n, now, now.saturating_sub(replied));
                    probe = Probe {
                        due: Some((now.saturating_add(LATENCY_TIMER_US), false)),
                        lease,
                        badge,
                        ..probe
                    };
                    wait.min(LATENCY_TIMER_US)
                }
                None => wait,
            };
            // The decision's, the same way, once the timer's wake and its line are behind.
            #[cfg(feature = "latency-probe")]
            let wait = if core::mem::take(&mut probe.decide) {
                let now = redoubt_rt::handle::time_now().unwrap_or(0);
                probe.due = Some((now.saturating_add(LATENCY_DECIDE_US), true));
                wait.min(LATENCY_DECIDE_US)
            } else {
                wait
            };
            match own.receive(wait, 0) {
                Ok(Event::Call(request)) if watches_call(&request.caller, &request.words) => {
                    // One per `sshd` instance; a restarted `sshd`'s comes before the abandoned
                    // one's notice. More is a fault of `sshd`'s, refused as it arrives.
                    if watches.len() < WATCHES {
                        watches.push(request);
                    }
                }
                Ok(Event::Call(mut request)) => {
                    let (caller, words, handles) = (request.caller, request.words, request.handles);
                    let began = redoubt_rt::handle::time_now().unwrap_or(0);
                    let mut serving = Serving::new(&mut steward, &mut machine);
                    let outcome = answer_with(&mut serving, &caller, &words, &handles, request.lend());
                    let (outputs, exited) = (core::mem::take(&mut serving.outputs), serving.exited);
                    let deferred = core::mem::take(&mut serving.after);
                    // The steward's own time on the two calls whose cost the responsiveness
                    // targets bound (kernel/scheduling.md, "Responsiveness"; servers/steward.md,
                    // "Residual risks": steward work is paid by the steward): `end_lease` and
                    // `submit`, from the call's arrival to its answer, said once each.
                    if let Some(op) = served_op(&words) {
                        let took = redoubt_rt::handle::time_now().unwrap_or(began).saturating_sub(began);
                        machine.out.say(&format!("steward: served {op} in {took} us\n"));
                    }
                    // A probe lease started: the timer is armed at the loop's next receive, the
                    // reply's time kept for the `reply_wait` sample.
                    #[cfg(feature = "latency-probe")]
                    if words[0] == 10 && outcome.words[0] == 0 {
                        if let Some(lease) = probe_lease(&steward, caller.badge) {
                            let replied = redoubt_rt::handle::time_now().unwrap_or(0);
                            probe.pending = Some((lease, caller.badge, replied));
                        }
                    }
                    // What the call did is said before it is answered, so its records come
                    // before anything the caller says of the answer: a turn of the console
                    // queue, one write per line within the bound; a line the console did not
                    // take waits for the loop's turn, and the reply does not.
                    after(machine.out, &mut steward, &mut usage, Some(outputs));
                    machine.out.turn(redoubt_rt::handle::time_now().unwrap_or(began));
                    let _ = finish(request, &outcome);
                    // A console a refused login or approval open brought is not kept.
                    machine.console(None, None);
                    machine.approval_consoles(None, None);
                    if exited {
                        return core_exited(machine.out);
                    }
                    // An approval channel's first screen, now that its `sshd` thread is free to
                    // take the write.
                    for kind in deferred {
                        let outputs = steward.event(&mut machine, kind, 0).ok();
                        if !after(machine.out, &mut steward, &mut usage, outputs) {
                            return core_exited(machine.out);
                        }
                    }
                }
                // A reader's report: its channel ended, and it waits for the next.
                Ok(Event::Send(delivery)) if delivery.caller.badge == READER_DONE => {
                    close_delivery(&delivery);
                    machine.readers.reported();
                }
                // A watcher's report: the exit endpoint it watched, now done with.
                Ok(Event::Send(delivery)) if delivery.caller.badge == EXITS => {
                    close_delivery(&delivery);
                    let key = delivery.words[0] as u64;
                    let _ = close(h(key as u32));
                    machine.watchers.reported();
                    // How the process ended, said once, under the name the notice gives it
                    // (`alice/agent-1`), so the two lines correlate: a session's VM has its own
                    // console and a lease's has none (servers/steward.md, "Leases"). A process
                    // whose object the core let go of first (a lease `end_lease` ended, its VM
                    // killed with its budget) gets its line and no event.
                    let cause = match delivery.words[1] {
                        1 => "exited",
                        2 => "faulted",
                        3 => "was killed",
                        _ => "ended",
                    };
                    let (object, name, running) = match steward.exited(key) {
                        Some(object) => {
                            let name = steward
                                .lease_name(object.id)
                                .unwrap_or_else(|| redoubt_steward_server::drive::named(&object));
                            (object, name, true)
                        }
                        None => match steward.ended(key) {
                            Some((object, name)) => (object, name, false),
                            None => continue,
                        },
                    };
                    // A crossing's program is named by its crossing: it ends with its budget's
                    // destruction once its one item is done ("Declassification and push").
                    let what = if object.kind == redoubt_steward::effect::Kind::Crossing {
                        "crossing"
                    } else {
                        "lease"
                    };
                    let line = format!(
                        "steward: {what} {name} of {}/{} {cause} {}\n",
                        object.domain.account(),
                        labels(object.domain.labels().as_slice()),
                        delivery.words[2]
                    );
                    machine.out.say(&line);
                    if !running {
                        continue;
                    }
                    // A lease the kernel's deadline took: its notice's arrival against the
                    // deadline, while the core still holds the lease.
                    #[cfg(feature = "latency-probe")]
                    if object.kind == redoubt_steward::effect::Kind::Lease && delivery.words[1] == 3 {
                        let deadline = inspect::domain(&steward.store, &object.domain)
                            .and_then(|d| d.leases.get(&object.id))
                            .map(|l| l.deadline);
                        let now = redoubt_rt::handle::time_now().unwrap_or(0);
                        if let Some(deadline) = deadline.filter(|d| *d <= now) {
                            let n = others(&steward, object.id);
                            probe.sample(machine.out, "deadline_notice", n, now, now - deadline);
                        }
                    }
                    let outputs = steward.event(&mut machine, EventKind::Exited { object }, 0).ok();
                    if !after(machine.out, &mut steward, &mut usage, outputs) {
                        return core_exited(machine.out);
                    }
                }
                Ok(Event::Send(delivery)) => close_delivery(&delivery),
                // A held `watch` whose caller is gone: an `sshd` that ended, and every channel
                // with it, so every attached context is detached (servers/steward.md, R80).
                Ok(Event::Abandoned(id)) => {
                    let before = watches.len();
                    watches.retain(|w| w.id() != id);
                    if watches.len() < before {
                        let outputs = steward.event(&mut machine, EventKind::SshdGone, 0).ok();
                        if !after(machine.out, &mut steward, &mut usage, outputs) {
                            return core_exited(machine.out);
                        }
                    }
                }
                Ok(_) => {}
                // The probe's wake: the timer's, then the decision's, on which the probe lease
                // ends by the steward's own event.
                #[cfg(feature = "latency-probe")]
                Err(Error::Timeout)
                    if probe.due.is_some_and(|(at, _)| at <= redoubt_rt::handle::time_now().unwrap_or(0)) =>
                {
                    let Some((at, deciding)) = probe.due.take() else { continue };
                    let woke = redoubt_rt::handle::time_now().unwrap_or(at);
                    let n = others(&steward, probe.lease);
                    if !deciding {
                        probe.sample(machine.out, "timer_wake", n, woke, woke.saturating_sub(at));
                        probe.decide = true;
                        continue;
                    }
                    probe.sample(machine.out, "decision_wake", n, woke, woke.saturating_sub(at));
                    machine.last_destroy = None;
                    let (badge, lease) = (probe.badge, probe.lease);
                    let outputs = steward.event(&mut machine, EventKind::EndLease { badge, lease }, 0).ok();
                    if let Some((end, gross)) = machine.last_destroy.take() {
                        probe.sample(machine.out, "destroy_call", n, end, gross);
                    }
                    if !after(machine.out, &mut steward, &mut usage, outputs) {
                        return core_exited(machine.out);
                    }
                }
                // The probe's look for the round's marker, half a second on: the loop's top
                // looks again.
                #[cfg(feature = "latency-probe")]
                Err(Error::Timeout) if !probe.counts.is_empty() && leases_live(&steward) == 0 => {}
                // The timers' own wakes: the loop's top ends what is due, and runs the cleanup
                // steps whose retry has come.
                Err(Error::Timeout) if [idle, due, line_due].into_iter().flatten().any(|t| t <= wait) => {}
                #[cfg(feature = "label-probe")]
                Err(Error::Timeout) if machine.probe_due.is_some() => machine.probe_hand(),
                #[cfg(feature = "restart-probe")]
                Err(Error::Timeout) => return PROBE_EXIT,
                Err(Error::Dead) => return redoubt_rt::exit::OK,
                Err(_) => return redoubt_rt::exit::RECEIVE_FAILED,
            }
        }
    }
}

redoubt_rt::entry!(machine::serve);
