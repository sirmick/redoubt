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

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

#[cfg(target_os = "none")]
mod machine {
    use alloc::collections::{BTreeMap, BTreeSet};
    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::num::NonZeroU64;
    use core::sync::atomic::{AtomicU32, Ordering};

    use redoubt_client::file::Connection;
    use redoubt_client::grants::RELEASE_TIMEOUT;
    use redoubt_client::launch::Launch;
    use redoubt_client::typed;
    use redoubt_rt::abi::{BudgetSpec, Error, FOREVER, Handle, MAX_LEND_PAGES};
    use redoubt_rt::client::{Connection as Nine, Lend};
    use redoubt_rt::handle::{Budget, Endpoint, close};
    use redoubt_rt::ipc::{Buffer, Event, Request};
    use redoubt_rt::server::ninep::mode;
    use redoubt_rt::server::typed::finish;
    use redoubt_rt::server::{close_delivery, own_args};
    use redoubt_rt::startup::Startup;
    use redoubt_rt::wire::proto::{consol, consrelay, ipd};
    use redoubt_steward::domain::Domain;
    use redoubt_steward::effect::Output;
    use redoubt_steward::event::EventKind;
    use redoubt_steward_server::own::{Bound, How, Own, SYSTEM, binding, session_args};
    use redoubt_steward_server::protocol::{Serving, answer_with, watches as watches_call};
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

    /// The program a session runs, on `/boot`, and its start module.
    const PROGRAM: &str = "beamlet";
    /// A context's console relay, on `/boot` (servers/consrelay.md).
    const RELAY: &str = "consrelay";
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
    /// A session VM's first-thread stack, in pages: twice beamlet's measured peak, 35,288 bytes
    /// (servers/init.md, "Stacks"; docs/testbench.md, "The memory budget").
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
    /// The steward's lend, in pages: the most a call may lend, so a 64-page batch of a session's
    /// image is read from `bootfsd` in four reads, not sixteen or more (a 9P read carries at most
    /// the lend's `iounit`, and a call costs about the same whatever it carries).
    const LEND_PAGES: usize = MAX_LEND_PAGES;

    /// The handle the watchers report on: `EXITS` minted on the steward's endpoint.
    static REPORT: AtomicU32 = AtomicU32::new(0);

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
            loop {
                match exit.receive(FOREVER, 0) {
                    Ok(Event::Exit(_)) | Err(_) => break,
                    Ok(Event::Call(request)) => drop(request),
                    Ok(Event::Send(delivery)) => close_delivery(&delivery),
                    Ok(_) => {}
                }
            }
            let report = Endpoint::from_handle(h(REPORT.load(Ordering::Acquire)));
            let _ = report.send(&[u64::from(index), 0, 0, 0], &[], None, FOREVER);
        }
    }

    /// What a connection the steward holds for a child was minted through: a server it was
    /// handed, by handle name, or a connection it carved and keeps, by its key.
    enum Via {
        Server(String),
        Carved(String),
    }

    /// What a connection the steward holds for a child was minted through, and the id it is
    /// disconnected by there; none for one the steward did not make (a badge, a console).
    type Held = Option<(Via, u64)>;

    struct Machine<'s> {
        startup: &'s Startup<'s>,
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
        held: BTreeMap<u32, Held>,
        /// The connections carved once with a quota and kept for the steward's life, by key (a
        /// principal's home): every session's is minted through one with no quota of its own, so
        /// they share it. `init` disconnects them at the steward's exit, with all minted below.
        carved: BTreeMap<String, Handle>,
        /// The held handles that are a session's console, told `ended` when they are released.
        consoles: BTreeSet<u32>,
        /// Each principal's account and name.
        accounts: Vec<(u64, String)>,
        own_lines: Own,
        /// A session's pages, the manifest's `sizes`: its VM's and its relay's.
        session_pages: u64,
        /// The program's length on `/boot`, and the relay's.
        program_len: usize,
        relay_len: usize,
        /// The endpoint relays say hello on, and the badge the last one was given.
        hello: Option<Endpoint>,
        hellos: u64,
        /// The VM's console the last relay served, for the next console slot.
        relay_vm: Option<Handle>,
        /// `label-probe` only: the first labelled context's relay, which every later attach of
        /// another relay hands its channel's console too.
        #[cfg(feature = "label-probe")]
        probe_victim: Option<Handle>,
        /// `label-probe` only: the hand-over due, at this time, of this channel console.
        #[cfg(feature = "label-probe")]
        probe_due: Option<(u64, Handle)>,
        /// The watchers' work endpoint, and the badge the steward sends them work on.
        work: Endpoint,
        work_send: Endpoint,
        watchers: Watchers,
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
            say(self.startup, &line);
        }

        fn server(&mut self, name: &str) -> Result<Connection, Error> {
            if let Some(conn) = self.servers.get(name) {
                return Ok(conn.clone());
            }
            let handle = self.startup.handle(name).ok_or(Error::BadHandle)?;
            let conn = Connection::attach(Endpoint::from_handle(handle), &mut self.lend)
                .map_err(|_| Error::Refused)?;
            self.servers.insert(name.into(), conn.clone());
            Ok(conn)
        }

        /// The server the steward was handed as `name`, unattached: minting and disconnecting
        /// need no fid, and the steward may not read a labelled volume's root itself (R25).
        fn unattached(&self, name: &str) -> Result<Nine, Error> {
            let handle = self.startup.handle(name).ok_or(Error::BadHandle)?;
            Ok(Nine::new(Endpoint::from_handle(handle)))
        }

        /// A fresh connection to `name`'s server rooted at `root`, held until released.
        fn fresh(&mut self, name: &str, root: &str) -> Result<Option<Handle>, Error> {
            let server = self.unattached(name)?;
            let (endpoint, id) =
                server.new_connection(&mut self.lend, root, 0).map_err(|_| Error::Refused)?;
            self.held.insert(endpoint.handle().index(), Some((Via::Server(name.into()), id)));
            Ok(Some(endpoint.handle()))
        }

        /// The connection kept for `key`, carved at `name`'s server rooted at `root` with `quota`
        /// bytes the first time it is needed.
        fn carve(&mut self, key: &str, name: &str, root: &str, quota: u64) -> Result<Nine, Error> {
            if let Some(handle) = self.carved.get(key) {
                return Ok(Nine::new(Endpoint::from_handle(*handle)));
            }
            let server = self.unattached(name)?;
            let (endpoint, _) =
                server.new_connection(&mut self.lend, root, quota).map_err(|_| Error::Refused)?;
            self.carved.insert(key.into(), endpoint.handle());
            Ok(Nine::new(endpoint))
        }

        /// A fresh connection at the root of the one kept for `key`, sharing its quota, held
        /// until released.
        fn fresh_carved(
            &mut self,
            key: &str,
            name: &str,
            root: &str,
            quota: u64,
        ) -> Result<Option<Handle>, Error> {
            let kept = self.carve(key, name, root, quota)?;
            let (endpoint, id) = kept.new_connection(&mut self.lend, "", 0).map_err(|_| Error::Refused)?;
            self.held.insert(endpoint.handle().index(), Some((Via::Carved(key.into()), id)));
            Ok(Some(endpoint.handle()))
        }

        fn principal(&self, domain: &Domain) -> Option<String> {
            let a = domain.account().get();
            self.accounts.iter().find(|(acc, _)| *acc == a).map(|(_, n)| n.clone())
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

        /// Keeps `handle` until it is released, with nothing to disconnect.
        fn keep(&mut self, handle: Handle) -> Handle {
            self.held.insert(handle.index(), None);
            handle
        }

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
        type Handle = Handle;

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

        fn destroy(&mut self, budget: Handle) -> Result<(), Error> { Budget::from_handle(budget).destroy() }

        fn budget_id(&self, budget: Handle) -> u64 { u64::from(budget.index()) }

        fn mint(&mut self, badge: u64, stamp: Handle) -> Result<Handle, Error> {
            let badge = NonZeroU64::new(badge).ok_or(Error::InvalidArgument)?;
            let minted = self.own.mint(badge, Some(&Budget::from_handle(stamp)))?;
            Ok(self.keep(minted.handle()))
        }

        fn connect(&mut self, domain: &Domain, slot: u16) -> Result<Option<Handle>, Error> {
            let principal = self.principal(domain).ok_or(Error::BadHandle)?;
            let Some(bound) = binding(&self.own_lines, &principal, domain.labels().as_slice(), slot) else {
                return Ok(None);
            };
            match bound.how {
                How::Fresh { server, root } => self.fresh(&server, &root),
                How::Carved { key, server, root, quota } => self.fresh_carved(&key, &server, &root, quota),
                How::Grant { scope } => {
                    let conn = self.server("ipd")?;
                    let grant = ipd::Message::Grant(ipd::Grant { scope: &scope });
                    let made = typed::call::<ipd::Protocol, _>(
                        conn.endpoint(),
                        &mut self.lend,
                        &grant,
                        &[],
                        |r, got| match r {
                            ipd::Reply::Grant(g) => got.take(0).map(|h| (h, g.id)),
                            _ => None,
                        },
                    );
                    let (handle, id) = made.ok().flatten().ok_or(Error::Refused)?;
                    self.held.insert(handle.index(), Some((Via::Server(String::from("ipd")), id)));
                    Ok(Some(handle))
                }
                How::Console => {
                    // A context's VM gets the console its relay serves; the relay holds the
                    // channel's.
                    if let Some(vm) = self.relay_vm.take() {
                        return Ok(Some(self.keep(vm)));
                    }
                    let console = match (self.console.take(), self.consoled.clone()) {
                        (Some(handle), _) => handle,
                        (None, Some(cons)) => {
                            let (endpoint, _) =
                                cons.new_connection(&mut self.lend, "", 0).map_err(|_| Error::Refused)?;
                            endpoint.handle()
                        }
                        (None, None) => return Ok(None),
                    };
                    self.consoles.insert(console.index());
                    Ok(Some(self.keep(console)))
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

        fn release(&mut self, handle: Handle) {
            // A session's console hears that the session is over: `sshd` ends its channel,
            // `consoled` releases the connection (servers/steward.md, "Authentication and sessions").
            if self.consoles.remove(&handle.index()) {
                if let Ok(ended) = consol::Message::Ended(consol::Ended {}).encode(&mut []) {
                    let _ = Endpoint::from_handle(handle).send(&ended, &[], None, RELEASE_TIMEOUT);
                }
            }
            if let Some(Some((via, id))) = self.held.remove(&handle.index()) {
                let through = match via {
                    Via::Server(name) => self.unattached(&name).ok(),
                    Via::Carved(key) => self.carved.get(&key).map(|h| Nine::new(Endpoint::from_handle(*h))),
                };
                if let Some(through) = through {
                    let _ = through.disconnect(id, RELEASE_TIMEOUT);
                }
            }
            let _ = close(handle);
        }

        fn launch(
            &mut self,
            domain: &Domain,
            budget: Handle,
            connections: &[Option<Handle>],
            context: Option<&str>,
        ) -> Result<u64, Error> {
            let principal = self.principal(domain).ok_or(Error::BadHandle)?;
            let labels = domain.labels().as_slice();
            let bound: Vec<Option<Bound>> =
                (0..SLOTS).map(|s| binding(&self.own_lines, &principal, labels, s)).collect();
            let exit = Endpoint::create()?;
            let exit_handle = exit.handle();
            // The VM's share: the session's pages less its relay's.
            let vm_pages = self.session_pages.saturating_sub(RELAY_PAGES);
            let pages = format!("budget_pages={vm_pages}");
            let told = session_args(&self.own_lines, &principal, labels, context);
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
                for (i, b) in bound.iter().enumerate() {
                    let (Some(b), Some(conn)) = (b, slot(i)) else { continue };
                    if let Some(name) = b.name {
                        launch.handle(name, conn);
                    }
                    if let Some(at) = &b.at {
                        launch.namespace(at, conn);
                    }
                }
                launch.stack_pages(SESSION_STACK_PAGES).heap_pages(u32::try_from(heap).unwrap_or(u32::MAX));
                told.iter()
                    .fold(launch.arg(&pages).arg(&endpoint).arg(&schedulers), |l, arg| l.arg(arg))
                    .arg(SHELL);
                launch.start()
            };
            let job = started.map_err(|failed| {
                let _ = close(exit_handle);
                match failed.error {
                    redoubt_client::Error::Sys(e) => e,
                    _ => Error::Refused,
                }
            })?;
            // The process ends with its budget; its notice comes to the exit endpoint.
            let _ = close(job.process().handle());
            if let Err(e) = self.watch_exit(exit_handle) {
                let _ = close(exit_handle);
                return Err(e);
            }
            Ok(u64::from(exit_handle.index()))
        }

        /// The relay is launched from `/boot` in the context's budget with a hello badge of its
        /// own, and the launch waits for its hello: the VM's console, kept for the next console
        /// slot, and the control connection. Its exit is watched as a session's, so a relay that
        /// dies ends its context. The badge carries the steward's stamp, not the context's: the
        /// kernel stamps a mint only at or below the endpoint's own (kernel/abi.md, `mint`), and
        /// the relay's copy goes with the relay's process.
        fn launch_relay(&mut self, domain: &Domain, budget: Handle) -> Result<(u64, Handle), Error> {
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
                launch.stack_pages(RELAY_STACK_PAGES).heap_pages(RELAY_HEAP_PAGES);
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
            let control = self.keep(control);
            #[cfg(feature = "label-probe")]
            if !domain.labels().is_empty() && self.probe_victim.is_none() {
                self.probe_victim = Some(control);
            }
            #[cfg(not(feature = "label-probe"))]
            let _ = domain;
            Ok((u64::from(exit_handle.index()), control))
        }

        /// The login's channel console is kept, told `ended` when it is released, and the relay
        /// is given the channel's other connection, `sshd`'s for it, with `note`. Only the kept
        /// one can end the channel (servers/sshd.md, "A pty session").
        fn attach(&mut self, relay: Handle, note: &str) -> Result<Handle, Error> {
            let (Some(console), Some(theirs)) = (self.console.take(), self.relay_console.take()) else {
                self.console(None, None);
                return Err(Error::BadHandle);
            };
            self.consoles.insert(console.index());
            let console = self.keep(console);
            let m = consrelay::Message::Attach(consrelay::Attach { note });
            let told = typed::call_within::<consrelay::Protocol, _>(
                &Endpoint::from_handle(relay),
                &mut self.lend,
                &m,
                &[theirs],
                RELAY_US,
                |_, _| (),
            );
            // The relay holds its copy now, or none: the steward keeps only its own.
            let _ = close(theirs);
            if told.is_err() {
                self.release(console);
                return Err(Error::Refused);
            }
            #[cfg(feature = "label-probe")]
            self.probe_cross(relay, console);
            Ok(console)
        }

        /// The relay writes `note` to its channel and lets it go, within [`RELAY_US`]: past it,
        /// `Timeout`, which the core's driver counts as detached.
        fn detach(&mut self, relay: Handle, note: &str) -> Result<(), Error> {
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

        fn random(&mut self) -> Result<u64, Error> { redoubt_rt::handle::random_u64() }

        fn now(&mut self) -> u64 { redoubt_rt::handle::time_now().unwrap_or(0) }
    }

    /// Says `line` on the console `init` gave the steward, if it has one. Not the runtime's
    /// `start::say`: both attach at fid 0, but that one clunks fid 0 after, and the steward holds
    /// its own attached connection there (`Machine::consoled`), through which it mints the console
    /// session's; the client library's attach never clunks it.
    fn say(startup: &Startup, line: &str) {
        let Some((_, console)) = startup.namespace().find(|(path, _)| *path == "/dev/cons") else { return };
        let Ok(mut lend) = Lend::new(1) else { return };
        let Ok(console) = Connection::attach(Endpoint::from_handle(console), &mut lend) else { return };
        if let Ok(file) = console.open(&mut lend, "", mode::OWRITE) {
            let _ = file.write_at(&mut lend, 0, line.as_bytes());
            let _ = file.close(&mut lend);
        }
    }

    fn labels(l: &[u64]) -> String {
        let items: Vec<String> = l.iter().map(|x| format!("{x}")).collect();
        format!("{{{}}}", items.join(","))
    }

    /// Audit records are console lines until the audit file (M3); so is a console session the
    /// steward could not open.
    fn deliver(startup: &Startup, outputs: Vec<Output>) {
        for o in outputs {
            if let Output::Reply { answer, .. } = &o {
                say(startup, &format!("steward: the console session did not open: {answer:?}\n"));
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
                say(startup, &line);
            }
        }
    }

    /// Says each batch step that failed, with the kernel's error: the core's answer names only
    /// `Failed`.
    fn failed(startup: &Startup, steward: &mut Steward<Handle, Handle>) {
        for (step, error) in core::mem::take(&mut steward.failed) {
            say(startup, &format!("steward: a step failed ({error:?}): {step:?}\n"));
        }
    }

    /// What an event did, said: its failed steps, its outputs, and the sub-budgets that changed;
    /// with no outputs, false: the core exited.
    fn after(
        startup: &Startup,
        steward: &mut Steward<Handle, Handle>,
        usage: &mut BTreeMap<u32, (u64, u64)>,
        outputs: Option<Vec<Output>>,
    ) -> bool {
        let Some(outputs) = outputs else { return false };
        failed(startup, steward);
        deliver(startup, outputs);
        tree(startup, steward, usage);
        true
    }

    /// The core exited: said once, and the steward's exit code.
    fn core_exited(startup: &Startup) -> u32 {
        say(startup, "steward: the core exited\n");
        CORE_EXITED
    }

    /// What each domain's sub-budget holds, said when it changes: the kernel's count of the
    /// pages and processes carved from it, which a session's budget takes and its end gives back.
    fn tree(startup: &Startup, steward: &Steward<Handle, Handle>, last: &mut BTreeMap<u32, (u64, u64)>) {
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
                    say(startup, &line);
                }
            }
        }
    }

    /// Serves until the endpoint is destroyed.
    pub fn serve(startup: &Startup) -> u32 {
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
            say(startup, "steward: cannot make its watchers' work endpoint\n");
            return NOT_STARTED;
        };
        let mut machine = Machine {
            startup,
            lend,
            own: Endpoint::from_handle(endpoint),
            servers: BTreeMap::new(),
            consoled: None,
            console: None,
            relay_console: None,
            held: BTreeMap::new(),
            carved: BTreeMap::new(),
            consoles: BTreeSet::new(),
            accounts: Vec::new(),
            own_lines: Own::default(),
            session_pages: 0,
            program_len: 0,
            relay_len: 0,
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
        };
        // The kernel's count of what `users` holds as this instance starts: 0, at boot and after
        // `init` emptied a dead steward's carves.
        if let Ok(u) = Budget::from_handle(users).usage() {
            say(
                startup,
                &format!("steward: users holds {} pages, {} processes\n", u.pages_usage, u.processes_usage),
            );
        }
        let mut steward: Steward<Handle, Handle> = match start(&lines, users, &mut machine) {
            Ok(s) => s,
            Err(e) => {
                say(startup, &format!("{e}\n"));
                return NOT_STARTED;
            }
        };
        for c in &steward.carved {
            let subs: Vec<String> = c.subs.iter().map(|(d, _)| labels(d.labels().as_slice())).collect();
            say(
                startup,
                &format!("steward: carved users/{} account {}: {}\n", c.name, c.account, subs.join(" ")),
            );
        }
        machine.accounts = steward.carved.iter().map(|c| (c.account, c.name.clone())).collect();
        machine.own_lines = steward.own.clone();
        machine.session_pages = steward.sizes.session.pages;
        if let Some((_, cons)) = startup.namespace().find(|(path, _)| *path == "/dev/cons") {
            machine.consoled = Connection::attach(Endpoint::from_handle(cons), &mut machine.lend).ok();
        }
        // A box with no program for its sessions starts them all failed: the steward still runs.
        if let Ok(conn) = machine.server("bootfsd") {
            if let Ok(stat) = conn.stat(&mut machine.lend, PROGRAM) {
                machine.program_len = stat.length as usize;
            }
            if let Ok(stat) = conn.stat(&mut machine.lend, RELAY) {
                machine.relay_len = stat.length as usize;
            }
        }
        let Ok(report) = own.mint(NonZeroU64::new(EXITS).expect("EXITS is not 0"), None) else {
            say(startup, "steward: cannot mint its exit reports' badge\n");
            return NOT_STARTED;
        };
        REPORT.store(report.handle().index(), Ordering::Release);
        let mut usage = BTreeMap::new();
        tree(startup, &steward, &mut usage);
        let outputs = steward.open_console(&mut machine).ok();
        if !after(startup, &mut steward, &mut usage, outputs) {
            return core_exited(startup);
        }
        #[cfg(feature = "restart-probe")]
        let probe_at = redoubt_rt::handle::time_now().unwrap_or(0).saturating_add(PROBE_DELAY);
        // `sshd`'s `watch` calls, held unanswered while this instance runs: its end fails each
        // with `Dead` (R4b), and so tells `sshd` its sessions are over.
        let mut watches: Vec<Request> = Vec::new();
        loop {
            // The idle timer: a detached context past its principal's bound ends (servers/steward.md,
            // "Contexts"); the receive below waits no longer than the next one's bound.
            let now = redoubt_rt::handle::time_now().unwrap_or(0);
            if steward.store.next_idle().is_some_and(|t| t <= now) {
                let outputs = steward.event(&mut machine, EventKind::Idle, 0).ok();
                if !after(startup, &mut steward, &mut usage, outputs) {
                    return core_exited(startup);
                }
            }
            let idle = steward.store.next_idle().map(|t| t.saturating_sub(now).max(1));
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
            let wait = idle.map_or(wait, |i| wait.min(i));
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
                    let mut serving = Serving::new(&mut steward, &mut machine);
                    let outcome = answer_with(&mut serving, &caller, &words, &handles, request.lend());
                    let (outputs, exited) = (core::mem::take(&mut serving.outputs), serving.exited);
                    // What the call did is said before it is answered, so its records come
                    // before anything the caller says of the answer.
                    after(startup, &mut steward, &mut usage, Some(outputs));
                    let _ = finish(request, &outcome);
                    // A console a refused login brought is not kept.
                    machine.console(None, None);
                    if exited {
                        return core_exited(startup);
                    }
                }
                // A watcher's report: the exit endpoint it watched, now done with.
                Ok(Event::Send(delivery)) if delivery.caller.badge == EXITS => {
                    close_delivery(&delivery);
                    let key = delivery.words[0] as u64;
                    let _ = close(h(key as u32));
                    machine.watchers.reported();
                    let Some(object) = steward.exited(key) else { continue };
                    let outputs = steward.event(&mut machine, EventKind::Exited { object }, 0).ok();
                    if !after(startup, &mut steward, &mut usage, outputs) {
                        return core_exited(startup);
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
                        if !after(startup, &mut steward, &mut usage, outputs) {
                            return core_exited(startup);
                        }
                    }
                }
                Ok(_) => {}
                // The timer's own wake: the loop's top ends what is due.
                Err(Error::Timeout) if idle.is_some_and(|i| i <= wait) => {}
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
