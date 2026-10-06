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
//! | 1 | the home volume's server, rooted at the home | at the home's path | the same; the server refuses its writes (R25) |
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
    use redoubt_rt::abi::{BudgetSpec, Error, FOREVER, Handle};
    use redoubt_rt::client::{Connection as Nine, Lend};
    use redoubt_rt::handle::{Budget, Endpoint, close};
    use redoubt_rt::ipc::{Buffer, Event};
    use redoubt_rt::server::ninep::mode;
    use redoubt_rt::server::typed::finish;
    use redoubt_rt::server::{close_delivery, own_args};
    use redoubt_rt::start::say;
    use redoubt_rt::startup::Startup;
    use redoubt_rt::wire::proto::{consol, ipd};
    use redoubt_steward::domain::Domain;
    use redoubt_steward::effect::Output;
    use redoubt_steward::event::EventKind;
    use redoubt_steward_server::own::{Bound, How, Own, SYSTEM, binding};
    use redoubt_steward_server::protocol::{Serving, answer_with};
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

    /// The program a session runs, on `/boot`, and its start module.
    const PROGRAM: &str = "beamlet";
    const SHELL: &str = "Elixir.Redoubt.Shell";
    /// A session VM's first-thread stack, in pages: twice beamlet's measured peak, 35,288 bytes
    /// (servers/init.md, "Stacks"; docs/testbench.md, "The memory budget").
    const SESSION_STACK_PAGES: usize = 18;
    /// A root badge of the steward's own, below the minted range and none of the protocol's
    /// roles: each session's watcher reports its exit on it.
    const EXITS: u64 = 4;
    /// A watcher's stack, in pages, as `init`'s.
    const WATCH_STACK_PAGES: usize = 4;
    /// The badge the steward sends a session's exit endpoint to an idle watcher under.
    const WORK: u64 = 1;
    /// The steward's lend, in pages: a batch of the program's image is read through it a page
    /// at a time.
    const LEND_PAGES: usize = 2;

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

    /// The server a connection the steward holds for a child came from, by handle name, and the
    /// id it is disconnected by; none for one the steward did not make (a badge, a console).
    type Held = Option<(String, u64)>;

    struct Machine<'s> {
        startup: &'s Startup<'s>,
        lend: Lend,
        own: Endpoint,
        /// Each server the steward was handed, attached once, by its handle name.
        servers: BTreeMap<String, Connection>,
        /// The steward's own console, from which the UART session's console is minted.
        consoled: Option<Connection>,
        /// The next login's console: `sshd`'s channel connection.
        console: Option<Handle>,
        held: BTreeMap<u32, Held>,
        /// The held handles that are a session's console, told `ended` when they are released.
        consoles: BTreeSet<u32>,
        /// Each principal's account and name.
        accounts: Vec<(u64, String)>,
        own_lines: Own,
        /// `budget_pages=` for a session's VM: the session's pages.
        session_pages: u64,
        /// The program's length on `/boot`.
        program_len: usize,
        /// The watchers' work endpoint, and the badge the steward sends them work on.
        work: Endpoint,
        work_send: Endpoint,
        watchers: Watchers,
    }

    impl Machine<'_> {
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
            self.held.insert(endpoint.handle().index(), Some((name.into(), id)));
            Ok(Some(endpoint.handle()))
        }

        fn principal(&self, domain: &Domain) -> Option<String> {
            let a = domain.account().get();
            self.accounts.iter().find(|(acc, _)| *acc == a).map(|(_, n)| n.clone())
        }

        /// Reads `buf.len()` bytes of the program at `at`, through `bootfsd`.
        fn read_program(&mut self, at: usize, buf: &mut [u8]) -> Result<(), Error> {
            let conn = self.server("bootfsd")?;
            let file = conn.open(&mut self.lend, PROGRAM, mode::OREAD).map_err(|_| Error::Refused)?;
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
    }

    impl Kernel for Machine<'_> {
        type Budget = Handle;
        type Handle = Handle;

        fn create(&mut self, parent: Handle, spec: &BudgetSpec) -> Result<Handle, Error> {
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
                    self.held.insert(handle.index(), Some((String::from("ipd"), id)));
                    Ok(Some(handle))
                }
                How::Console => {
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

        fn console(&mut self, handle: Option<Handle>) {
            if let Some(old) = core::mem::replace(&mut self.console, handle) {
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
            if let Some(Some((name, id))) = self.held.remove(&handle.index()) {
                if let Ok(server) = self.unattached(&name) {
                    let _ = server.disconnect(id, RELEASE_TIMEOUT);
                }
            }
            let _ = close(handle);
        }

        fn launch(
            &mut self,
            domain: &Domain,
            budget: Handle,
            connections: &[Option<Handle>],
        ) -> Result<u64, Error> {
            let principal = self.principal(domain).ok_or(Error::BadHandle)?;
            let labels = domain.labels().as_slice();
            let bound: Vec<Option<Bound>> =
                (0..SLOTS).map(|s| binding(&self.own_lines, &principal, labels, s)).collect();
            let exit = Endpoint::create()?;
            let exit_handle = exit.handle();
            let pages = format!("budget_pages={}", self.session_pages);
            let endpoint = format!("endpoint={SYSTEM}");
            let len = self.program_len;
            // The heap capped at what the session's budget holds beside the stack and the
            // process's own page (servers/init.md, "Heaps").
            let heap = self.session_pages.saturating_sub(SESSION_STACK_PAGES as u64 + 1);
            // The steward's badge first, then the shared slots in order.
            let slot = |i: usize| connections.get(i + 1).copied().flatten();
            let started = {
                let mut read = |at: usize, buf: &mut [u8]| self.read_program(at, buf);
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
                launch.arg(&pages).arg(&endpoint).arg(SHELL);
                launch.start()
            };
            let job = started.map_err(|failed| {
                let _ = close(exit_handle);
                match failed.error {
                    redoubt_client::Error::Sys(e) => e,
                    _ => Error::Refused,
                }
            })?;
            // The process ends with its budget; its notice comes to the exit endpoint, which an
            // idle watcher takes, or a new one when every watcher is watching.
            let _ = close(job.process().handle());
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
                    .send(&[u64::from(exit_handle.index()), 0, 0, 0], &[], None, FOREVER)
                    .map_err(|(e, _)| e)
            });
            // Unwatched, the session's notice would never be read: the launch is a failed step, as
            // a refused one is.
            if let Err(e) = watched {
                self.watchers.unwatch(new);
                let _ = close(exit_handle);
                return Err(e);
            }
            Ok(u64::from(exit_handle.index()))
        }

        fn random(&mut self) -> Result<u64, Error> { redoubt_rt::handle::random_u64() }

        fn now(&mut self) -> u64 { redoubt_rt::handle::time_now().unwrap_or(0) }
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

    /// What each domain's sub-budget holds, said when it changes: the kernel's count of the
    /// pages and processes carved from it, which a session's budget takes and its end gives back.
    /// What an event did, said: its failed steps, its outputs, and the sub-budgets that changed;
    /// or, with no outputs, that the core exited, and false.
    fn after(
        startup: &Startup,
        steward: &mut Steward<Handle, Handle>,
        usage: &mut BTreeMap<u32, (u64, u64)>,
        outputs: Option<Vec<Output>>,
    ) -> bool {
        let Some(outputs) = outputs else {
            say(startup, "steward: the core exited\n");
            return false;
        };
        failed(startup, steward);
        deliver(startup, outputs);
        tree(startup, steward, usage);
        true
    }

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
            held: BTreeMap::new(),
            consoles: BTreeSet::new(),
            accounts: Vec::new(),
            own_lines: Own::default(),
            session_pages: 0,
            program_len: 0,
            work,
            work_send,
            watchers: Watchers::default(),
        };
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
            return CORE_EXITED;
        }
        loop {
            match own.receive(FOREVER, 0) {
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
                    machine.console(None);
                    if exited {
                        say(startup, "steward: the core exited\n");
                        return CORE_EXITED;
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
                        return CORE_EXITED;
                    }
                }
                Ok(Event::Send(delivery)) => close_delivery(&delivery),
                Ok(_) => {}
                Err(Error::Dead) => return redoubt_rt::exit::OK,
                Err(_) => return redoubt_rt::exit::RECEIVE_FAILED,
            }
        }
    }
}

redoubt_rt::entry!(machine::serve);
