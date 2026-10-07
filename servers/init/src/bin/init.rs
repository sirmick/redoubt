//! `init`, the program: the one process the loader starts (kernel/boot.md, "The loader loads only
//! the kernel and `init`"). It reads the boot manifest from the bundle, checks all of it against
//! the machine ([`redoubt_init::check`]), and then starts the servers in the order
//! servers/init.md, "Starting the servers", gives: their endpoints, `keyd` and the key-separation
//! check, `consoled` with the UART `init` gave up, the rest, and the `public` entries pushed to
//! `bootfsd`. A refusal, from the check or from any step after it, is one line and a power-off
//! reporting a system failure, so no boot runs half started.
//!
//! From the first server on, a server that ends is restarted, and the boot's steps that called it
//! are run again on the new instance; one that cannot stay up reboots the machine
//! (servers/init.md, "Restarts and reboots").
//!
//! Its first moves are fixed by the bound on `root` (kernel/budgets.md, "The tree from the boot
//! manifest"): read `root`'s free pages first, then take the arena once, so the arena is counted
//! once, in the bound, and every allocation after it comes from the arena.

#![cfg_attr(target_os = "none", no_std, no_main)]
#![forbid(unsafe_code)]

#[cfg(target_os = "none")]
mod machine {
    extern crate alloc;

    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::fmt;
    use core::num::NonZeroU64;
    use core::sync::atomic::{AtomicU32, Ordering};

    use redoubt_client::Error as CallError;
    use redoubt_client::file::{Connection, File};
    use redoubt_client::grants::RELEASE_TIMEOUT;
    use redoubt_client::launch::{Job, Launch};
    use redoubt_client::typed;
    use redoubt_init::bound::{LEND_PAGES, WATCH_STACK_PAGES};
    use redoubt_init::bundle::{Bundle, Entry};
    use redoubt_init::check::{BOOTFSD, MANIFEST, Machine, Plan, USERS, VOLUME, args, is_steward, range};
    use redoubt_init::manifest::{DeviceUse, Manifest};
    use redoubt_init::refusal::Refusal;
    use redoubt_init::restarts::{self, Restarts};
    use redoubt_init::{ARENA_PAGES, check, read};
    use redoubt_rt::abi::{
        BudgetSpec, Cause, Error, FOREVER, Handle, Labels, MAX_LABELS, MAX_THREADS, PAGE_SIZE, ResetKind,
        Usage,
    };
    use redoubt_rt::client::Lend;
    use redoubt_rt::handle::{Budget, Endpoint, Mmio, Registers, Reset};
    use redoubt_rt::ipc::{Buffer, Event};
    use redoubt_rt::server::close_delivery;
    use redoubt_rt::server::ninep::mode;
    use redoubt_rt::wire::proto::{bootfs, keyd};
    use redoubt_sys::DeviceInfo;

    /// The stub's own flat binary (`build.rs`).
    static STUB_BIN: &[u8] = include_bytes!(env!("STUB_BIN"));

    /// The handles the kernel gives the first program (kernel/boot.md, "Devices handed to the first
    /// program"): the three budgets, the Reset right, the console's register region and interrupt,
    /// then every other device.
    const ROOT: u32 = 1;
    const SYSTEM: u32 = 2;
    const USERS_BUDGET: u32 = 3;
    const RESET: u32 = 4;
    const CONSOLE_MMIO: u32 = 5;
    /// The bytes of a public entry each `add` carries: a page, inside the lend with its name.
    const CHUNK: usize = PAGE_SIZE;

    /// The 16550's registers `init` writes before `consoled` starts: transmit, and the line status
    /// whose bit 5 says the transmitter can take a byte.
    const THR: usize = 0;
    const LSR: usize = 5;
    const LSR_THR_EMPTY: u8 = 1 << 5;

    /// The endpoint the watching threads report exits on, which the first thread receives on.
    static REPORTS: AtomicU32 = AtomicU32::new(0);
    /// Each server's running instance's budget, by server index (the check keeps the servers fewer
    /// than `MAX_THREADS`): its watching thread destroys it at the instance's end.
    static BUDGETS: [AtomicU32; MAX_THREADS] = [const { AtomicU32::new(0) }; MAX_THREADS];
    /// How many of each server's instances have ended. The watching thread counts an end before it
    /// destroys the budget, which sweeps every handle stamped with it, `init`'s own at the server
    /// among them; so a count read unchanged just before a call says the handle is still the one
    /// `init` made, not a slot swept and reused.
    static ENDED: [AtomicU32; MAX_THREADS] = [const { AtomicU32::new(0) }; MAX_THREADS];
    /// Each server's last exit notice's blame: the account, the labels' count, then the labels,
    /// each word as two halves, since rv32 has no 64-bit atomics.
    static BLAMED: [[AtomicU32; 2 * (2 + MAX_LABELS)]; MAX_THREADS] =
        [const { [const { AtomicU32::new(0) }; 2 * (2 + MAX_LABELS)] }; MAX_THREADS];

    fn keep(server: usize, at: usize, word: u64) {
        BLAMED[server][2 * at].store(word as u32, Ordering::SeqCst);
        BLAMED[server][2 * at + 1].store((word >> 32) as u32, Ordering::SeqCst);
    }

    fn kept(server: usize, at: usize) -> u64 {
        let half = |n: usize| u64::from(BLAMED[server][n].load(Ordering::SeqCst));
        half(2 * at) | (half(2 * at + 1) << 32)
    }

    fn h(index: u32) -> Handle { Handle::new(index).expect("a slot is at least 1") }

    /// ` [t=N]`, `time_now` in µs, after a line the boot profile times (docs/testbench.md,
    /// "Checked builds"). Only a `boot-stats` build has it: every other build, and every other
    /// case, has the line as it was.
    #[cfg(feature = "boot-stats")]
    fn stamp() -> String { format!(" [t={}]", redoubt_rt::handle::time_now().unwrap_or(0)) }

    /// Where `init`'s lines go: the UART until `consoled` starts, then `init`'s own connection to
    /// `consoled`; nowhere in between, or while `consoled` is down.
    enum Out {
        Uart(Registers),
        Console(File),
        Nowhere,
    }

    impl Out {
        /// `lend` carries the line to `consoled`; the UART needs none.
        fn write(&self, lend: Option<&mut Lend>, line: &str) {
            match self {
                Out::Uart(registers) => {
                    for byte in line.bytes() {
                        while registers.read_u8(LSR).is_some_and(|lsr| lsr & LSR_THR_EMPTY == 0) {}
                        registers.write_u8(THR, byte);
                    }
                }
                Out::Console(file) => {
                    if let Some(lend) = lend {
                        let _ = file.write_at(lend, 0, line.as_bytes());
                    }
                }
                Out::Nowhere => {}
            }
        }
    }

    /// Prints the refusal and powers the machine off reporting a system failure: every refusal,
    /// the check's or a step's after it, ends the boot here.
    fn refuse(out: &Out, lend: Option<&mut Lend>, why: &Refusal) -> ! {
        out.write(lend, &format!("init: refused the boot: {why}\n"));
        let _ = Reset::from_handle(h(RESET)).reset(ResetKind::PowerOffFailure);
        redoubt_rt::handle::process_exit(1)
    }

    /// `device_info` on each device handle from the Reset right on, until the first that is none.
    fn devices() -> Vec<(Handle, DeviceInfo)> {
        let mut out = Vec::new();
        for index in RESET.. {
            let Some(handle) = Handle::new(index) else { break };
            match redoubt_rt::handle::device_info(handle) {
                Ok(info) => out.push((handle, info)),
                Err(_) => break,
            }
        }
        out
    }

    /// The machine's side of the check: everything [`check`] compares the manifest with.
    fn machine<'a>(
        devices: &'a [(Handle, DeviceInfo)],
        entries: &'a [(&'a str, usize)],
        root: Usage,
    ) -> Result<Machine<'a>, Error> {
        Ok(Machine {
            devices,
            system: Budget::from_handle(h(SYSTEM)).usage()?,
            root,
            entries,
            stub_bytes: STUB_BIN.len(),
            arena_pages: ARENA_PAGES,
            // The budgets, then the devices from the Reset right on.
            handles_at_start: RESET as usize - 1 + devices.len(),
        })
    }

    /// The server whose program is `program`, by index: the check lets at most one run each
    /// program `init` calls.
    fn running(m: &Manifest, program: &str) -> Option<usize> {
        m.servers.iter().position(|s| s.program == program)
    }

    /// The manifest's names for `labels`, or the number of one it does not name.
    fn label_names(m: &Manifest, labels: &[u64]) -> String {
        if labels.is_empty() {
            return String::from("none");
        }
        let name = |id: &u64| match m.labels.iter().find(|l| l.id == *id) {
            Some(l) => l.name.clone(),
            None => format!("{id}"),
        };
        labels.iter().map(name).collect::<Vec<_>>().join(",")
    }

    /// One server's running instance.
    struct Started {
        /// `ENDED` for the server while this instance runs.
        ended_at: u32,
        /// Its console connection's id at `consoled`, and `consoled`'s `ENDED` when it was
        /// minted: disconnected at the exit, unless that `consoled` has ended and taken it.
        console: Option<(u64, u32)>,
        /// Kept, never waited on: the server's watching thread receives its exit notice and
        /// destroys its budget, which sweeps the job's handles.
        _job: Job,
    }

    /// Why a step that calls a server stopped.
    enum Stop {
        /// The server ended under it: the step waits for the new instance and runs again.
        Exited,
        /// Anything else: the boot is refused, or after a restart the machine reboots.
        Failed(Refusal),
    }

    /// The boot in progress, and the restarts after it.
    struct Boot<'a> {
        manifest: &'a Manifest,
        plan: &'a Plan,
        entries: &'a [Entry<'static>],
        out: Out,
        lend: Lend,
        /// Each endpoint a server receives on, by name: `init` keeps the receive right.
        endpoints: Vec<(&'a str, Endpoint)>,
        /// `init`'s own handle at each server it calls, by server index ([`Plan::init_badges`]),
        /// minted for each instance and stamped with its budget, so its end ends the handle.
        own: Vec<(usize, Endpoint)>,
        /// `init`'s own connection to `consoled`, once it runs, and `consoled`'s `ENDED` then.
        console: Option<Connection>,
        console_at: u32,
        /// Each server's running instance, by index; none between its exit and its restart.
        started: Vec<Option<Started>>,
        /// Each server's exit endpoint, made with its watching thread at its first start and
        /// kept for every instance after.
        exits: Vec<Option<Handle>>,
        /// Each server's last restarts, for the reboot rule.
        restarts: Vec<Restarts>,
        /// Whether a restart is under way: a step of it that fails reboots.
        restarting: bool,
        keyd: usize,
        consoled: Option<usize>,
        bootfsd: Option<usize>,
        /// Whether the boot has come to its public entries: a `bootfsd` restarted after that gets
        /// them again.
        public: bool,
        /// `root`'s pages in use before the arena was taken: what the bound is measured from.
        root_before: u64,
        /// The copies `init` closed once their children held them, each found gone.
        closed: Closed,
    }

    /// The copies of children's handles `init` closed and found gone ([`Boot::start`]).
    #[derive(Clone, Copy, Default)]
    struct Closed {
        badges: usize,
        consoles: usize,
    }

    impl<'a> Boot<'a> {
        fn say(&mut self, line: fmt::Arguments) {
            self.connection();
            self.out.write(Some(&mut self.lend), &format!("{line}\n"))
        }

        /// A line the boot profile times: [`Boot::say`], stamped in a `boot-stats` build.
        fn milestone(&mut self, line: fmt::Arguments) {
            #[cfg(feature = "boot-stats")]
            self.say(format_args!("{line}{}", stamp()));
            #[cfg(not(feature = "boot-stats"))]
            self.say(line);
        }

        /// A step after the checks failed: a bug in the bound or the checks. `at` names where. The
        /// boot is refused; a restart that fails reboots instead, as one that cannot stay up does.
        fn failed(&mut self, at: &str, step: &'static str) -> ! {
            let why = Refusal::failed(at, step);
            if self.restarting {
                self.reboot(format_args!("{why}"));
            }
            refuse(&self.out, Some(&mut self.lend), &why)
        }

        /// Prints why and reboots the machine: failing closed beats a server that cannot stay up.
        fn reboot(&mut self, why: fmt::Arguments) -> ! {
            self.say(format_args!("init: rebooting: {why}"));
            let _ = Reset::from_handle(h(RESET)).reset(ResetKind::Reboot);
            redoubt_rt::handle::process_exit(1)
        }

        /// `init`'s connection to `consoled`, unless the instance it was made at has ended: its
        /// handle may have been swept and its slot reused, so nothing more goes through it, and
        /// `init`'s lines go nowhere until it attaches to the next.
        fn connection(&mut self) -> Option<&Connection> {
            if let (Some(_), Some(c)) = (&self.console, self.consoled) {
                if ENDED[c].load(Ordering::SeqCst) != self.console_at {
                    self.console = None;
                    self.out = Out::Nowhere;
                }
            }
            self.console.as_ref()
        }

        /// The receive right of the endpoint `name`, which the checks found a server for.
        fn endpoint(&self, name: &str) -> Handle {
            self.endpoints
                .iter()
                .find(|(n, _)| *n == name)
                .expect("the check found every endpoint")
                .1
                .handle()
        }

        /// Whether server `i`'s instance is running: started, and its end not yet counted.
        fn alive(&self, i: usize) -> bool {
            self.started[i].as_ref().is_some_and(|s| ENDED[i].load(Ordering::SeqCst) == s.ended_at)
        }

        /// `init`'s own handle at server `i`, which [`Plan::init_badges`] names, while the instance
        /// it was minted for runs.
        fn own(&self, i: usize) -> Result<Endpoint, Stop> {
            if !self.alive(i) {
                return Err(Stop::Exited);
            }
            Ok(Endpoint::from_handle(
                self.own.iter().find(|(s, _)| *s == i).expect("init calls it").1.handle(),
            ))
        }

        /// What a failed call to server `i` means: `Dead` (it ended holding the call, or the
        /// sweep failed it queued) or an end already counted is an exit; anything else, `step`
        /// failed.
        fn stop(&self, i: usize, e: CallError, step: &'static str) -> Stop {
            if e == CallError::Disconnected || !self.alive(i) {
                Stop::Exited
            } else {
                Stop::Failed(Refusal::failed(&self.manifest.servers[i].name, step))
            }
        }

        /// Steps 2 to 6: the endpoints, `keyd` and the key check, `consoled`, the rest, the public
        /// entries and the steward, restarting any server that ends meanwhile; then the exits.
        fn run(&mut self) -> ! {
            let m = self.manifest;
            for s in &m.servers {
                for name in &s.receives {
                    let Ok(endpoint) = Endpoint::create() else {
                        self.failed(&s.name, "create its endpoint")
                    };
                    self.endpoints.push((name, endpoint));
                }
            }
            let Ok(reports) = Endpoint::create() else { self.failed("init", "create its reports endpoint") };
            REPORTS.store(reports.handle().index(), Ordering::Release);
            let keyd = self.keyd;
            self.start(keyd);
            self.settle(keyd, false);
            if let Some(i) = self.consoled {
                self.give_up_the_uart();
                self.start(i);
                self.settle(i, false);
            }
            let consoled = self.consoled;
            // The steward starts sessions from `/boot`, so it comes after the public entries.
            let steward = m.servers.iter().position(|s| is_steward(m, s));
            for i in (0..m.servers.len()).filter(|&i| i != keyd && Some(i) != consoled && Some(i) != steward)
            {
                self.start(i);
                self.drain();
            }
            if let Some(i) = self.bootfsd {
                self.public = true;
                self.settle(i, false);
            }
            if let Some(i) = steward {
                self.start(i);
                self.drain();
            }
            // What the boot cost `root`, against the bound the check passed it on: more is a bug
            // in the bound, and the boot is refused rather than kept on a bound that lied.
            let Ok(root) = Budget::from_handle(h(ROOT)).usage() else {
                self.failed("init", "read root's usage")
            };
            let used = root.pages_usage - self.root_before;
            if used > self.plan.bound {
                self.failed("init", "stay within its bound on root");
            }
            // Printed only when every copy was found gone: one that was not refused the boot.
            let Closed { badges, consoles } = self.closed;
            self.say(format_args!(
                "init: holds none of the {badges} badges and {consoles} console connections it handed"
            ));
            self.milestone(format_args!(
                "init: the boot is done: {} servers, root holds {used} pages for it, within the bound of {}",
                m.servers.len(),
                self.plan.bound
            ));
            loop {
                if let Some(i) = self.next_exit(FOREVER) {
                    self.settle(i, true);
                }
            }
        }

        /// Starts server `i` in a budget of its own, watched by its thread: the first instance, or
        /// a new one on the same endpoints, with new badges and a new console connection.
        fn start(&mut self, i: usize) {
            let m = self.manifest;
            let s = &m.servers[i];
            let labels: Vec<u64> = s
                .labels
                .iter()
                .filter_map(|n| m.labels.iter().find(|l| &l.name == n))
                .map(|l| l.id)
                .collect();
            let Ok(labels) = Labels::from_slice(&labels) else { self.failed(&s.name, "label the budget of") };
            // The check kept processes and weight within 1 to `u32::MAX`.
            let spec = BudgetSpec {
                pages: s.budget.pages,
                processes: s.budget.processes as u32,
                weight: s.budget.weight as u32,
                labels,
                account: 0,
                deadline: FOREVER,
            };
            let Ok(budget) = Budget::from_handle(h(SYSTEM)).create_child(&spec) else {
                self.failed(&s.name, "carve the budget of")
            };
            // Before the instance runs, so that its watching thread finds this budget at its end.
            BUDGETS[i].store(budget.handle().index(), Ordering::SeqCst);
            let ended_at = ENDED[i].load(Ordering::SeqCst);
            if let Some(&(_, badge)) = self.plan.init_badges.iter().find(|(s, _)| *s == i) {
                let badge = NonZeroU64::new(badge).expect("init's badge is at least 1");
                let at = Endpoint::from_handle(self.endpoint(&s.receives[0]));
                let Ok(own) = at.mint(badge, Some(&budget)) else {
                    self.failed(&s.name, "mint init's handle at")
                };
                self.own.retain(|(s, _)| *s != i);
                self.own.push((i, own));
            }
            let exit = match self.exits[i] {
                Some(exit) => Endpoint::from_handle(exit),
                None => {
                    let Ok(exit) = Endpoint::create() else {
                        self.failed(&s.name, "create the exit endpoint of")
                    };
                    exit
                }
            };
            let exit_handle = exit.handle();
            let image = self.entries.iter().find(|e| e.name == s.program).expect("the check found it").data;
            let mut console = None;
            if let Some(parent) = self.connection().cloned() {
                match parent.new_connection(&mut self.lend, "", 0) {
                    Ok(minted) => console = Some(minted),
                    // A `consoled` that ended meanwhile: this instance runs without a console.
                    Err(e) if matches!(self.consoled.map(|c| self.stop(c, e, "")), Some(Stop::Exited)) => {}
                    Err(_) => self.failed(&s.name, "mint a console connection for"),
                }
            }
            let mut handed = Vec::new();
            for item in &s.handed {
                let badge = NonZeroU64::new(item.badge).expect("the check refused badge 0");
                // Stamped with the instance's budget, as `init`'s own handle is: a copy the server
                // passes on dies with it, so its restart's badge has no other holder.
                let Ok(minted) =
                    Endpoint::from_handle(self.endpoint(&item.endpoint)).mint(badge, Some(&budget))
                else {
                    self.failed(&s.name, "mint a handed badge for")
                };
                handed.push((item.endpoint.as_str(), minted.handle()));
            }
            // A volume's range ([`range`]): at its disk's `blkd`, or at its verifier's endpoint
            // for a verified volume's server, minted again at every start, stamped as the handed
            // badges are.
            if let Some((endpoint, badge)) = range(m, s) {
                let badge = NonZeroU64::new(badge).expect("a range's badge is at least 1");
                let Ok(minted) = Endpoint::from_handle(self.endpoint(endpoint)).mint(badge, Some(&budget))
                else {
                    self.failed(&s.name, "mint the volume's range for")
                };
                handed.push((VOLUME, minted.handle()));
            }
            let args = args(m, s, &self.plan.bundle_key);
            let mut launch = Launch::new(STUB_BIN, image, budget, exit);
            launch.stack_pages(s.stack_pages as usize).stack_tag((i + 1) as u16);
            launch.heap_pages(s.heap_pages.unwrap_or(0));
            for name in &s.receives {
                launch.handle(name, self.endpoint(name));
            }
            for (name, minted) in &handed {
                launch.handle(name, *minted);
            }
            // `init`'s own copies: a restart places them again.
            for (name, device) in &self.plan.placements[i] {
                launch.handle(name, *device);
            }
            // The steward's entry alone gets `users`, which it carves the principals' budgets
            // from (step 6; R33). `init` keeps its own copy.
            if is_steward(m, s) {
                launch.handle(USERS, h(USERS_BUDGET));
            }
            if let Some((conn, _)) = &console {
                launch.namespace("/dev/cons", conn.handle());
            }
            for arg in &args {
                launch.arg(arg);
            }
            let Ok(job) = launch.start() else { self.failed(&s.name, "start") };
            // The child holds its own copies now. `init` keeps none of the badges it handed, nor
            // the child's console connection, so it calls as no system caller and writes as no
            // child (Authority). Each copy is probed at once, before `init` makes another handle
            // that could take its slot: a second close the kernel refuses as a bad handle shows
            // the copy is gone from the kernel's side.
            let copies = handed.iter().map(|(_, h)| *h).chain(console.as_ref().map(|(c, _)| c.handle()));
            for copy in copies {
                let gone = redoubt_rt::handle::close(copy).is_ok()
                    && redoubt_rt::handle::close(copy) == Err(Error::BadHandle);
                if !gone {
                    self.failed(&s.name, "close its copies of the handles handed to");
                }
            }
            self.closed.badges += handed.len();
            self.closed.consoles += usize::from(console.is_some());
            if self.exits[i].is_none() {
                let Ok(stack) = Buffer::new(WATCH_STACK_PAGES as usize) else {
                    self.failed(&s.name, "map the watching thread's stack for")
                };
                let watched = Watched { server: i, exit: exit_handle };
                if redoubt_rt::handle::thread_create(watch, stack, watched.word()).is_err() {
                    self.failed(&s.name, "start the watching thread for");
                }
                self.exits[i] = Some(exit_handle);
            }
            let id = console.map(|(_, id)| id);
            let verb = if ended_at == 0 { "started" } else { "restarted" };
            // `consoled` itself starts while `init` writes nowhere: `attach_console` says it.
            match id {
                // The id bare, as `consoled` prefixes the child's lines with it
                // (servers/consoled.md, "Started by `init`").
                Some(id) => self.milestone(format_args!("init: {verb} {}, console {id:016x}", s.name)),
                None if !matches!(self.out, Out::Nowhere) => {
                    self.milestone(format_args!("init: {verb} {}", s.name))
                }
                None => {}
            }
            let console = id.map(|id| (id, self.consoled.map_or(0, |c| ENDED[c].load(Ordering::SeqCst))));
            self.started[i] = Some(Started { ended_at, console, _job: job });
        }

        /// What the boot did after starting server `i`, done again for each new instance.
        fn step(&mut self, i: usize) -> Result<(), Stop> {
            if i == self.keyd {
                self.check_keys(i)
            } else if Some(i) == self.consoled {
                self.attach_console(i)
            } else if Some(i) == self.bootfsd && self.public {
                self.push_public(i)
            } else {
                Ok(())
            }
        }

        /// Runs server `i`'s step until it is done, waiting out each end of the server under it;
        /// a server another exit restarted meanwhile has its own step run at once. A step that
        /// fails otherwise refuses the boot, or reboots if a restart called for it.
        fn settle(&mut self, i: usize, restarted: bool) {
            loop {
                match self.step(i) {
                    Ok(()) => return,
                    Err(Stop::Exited) => loop {
                        let Some(j) = self.next_exit(FOREVER) else { continue };
                        if j == i {
                            break;
                        }
                        self.settle(j, true);
                    },
                    Err(Stop::Failed(why)) if restarted => self.reboot(format_args!("{why}")),
                    Err(Stop::Failed(why)) => refuse(&self.out, Some(&mut self.lend), &why),
                }
            }
        }

        /// Restarts every server whose end was reported, without waiting for one.
        fn drain(&mut self) {
            while let Some(i) = self.next_exit(0) {
                self.settle(i, true);
            }
        }

        /// Takes the next exit report within `timeout` µs and restarts its server, whose index it
        /// returns; none if none came.
        fn next_exit(&mut self, timeout: u64) -> Option<usize> {
            let reports = Endpoint::from_handle(h(REPORTS.load(Ordering::Acquire)));
            loop {
                let Ok(event) = reports.receive(timeout, 0) else { return None };
                let Event::Send(report) = event else { continue };
                let [server, pid, cause, code] = report.words;
                let Some(i) = usize::try_from(server)
                    .ok()
                    .filter(|&i| self.started.get(i).is_some_and(Option::is_some))
                else {
                    continue;
                };
                self.restart(i, pid, cause, code);
                return Some(i);
            }
        }

        /// Server `i`'s instance ended: its line, its console connection released, the reboot
        /// rules, and a new instance. Its watching thread destroyed its budget, which swept the
        /// job's handles and `init`'s own at it.
        fn restart(&mut self, i: usize, pid: u64, cause: u64, code: u64) {
            let m = self.manifest;
            let name = m.servers[i].name.as_str();
            let ended = self.started[i].take();
            if cause == Cause::Faulted as u64 {
                let account = kept(i, 0);
                let count = (kept(i, 1) as usize).min(MAX_LABELS);
                let labels: Vec<u64> = (0..count).map(|n| kept(i, 2 + n)).collect();
                let serving =
                    if account == 0 { String::from("nobody") } else { format!("account {account}") };
                let labels = label_names(m, &labels);
                self.say(format_args!(
                    "init: {name} (PID {pid}) faulted, code {code}, serving {serving}, labels {labels}; blamed on nobody: no steward"
                ));
            } else {
                let cause = if cause == Cause::Exited as u64 { "exited" } else { "was killed" };
                self.say(format_args!("init: {name} (PID {pid}) {cause}, code {code}"));
            }
            if let Some((id, at)) = ended.and_then(|s| s.console) {
                if let (Some(c), Some(console)) = (self.consoled, &self.console) {
                    if ENDED[c].load(Ordering::SeqCst) == at {
                        let _ = console.disconnect(id, RELEASE_TIMEOUT);
                    }
                }
            }
            // A device whose reset was never confirmed was destroyed at the driver's end, and
            // `init`'s copy with it: only a hardware reset makes it safe to hand out again.
            for (placed, handle) in &self.plan.placements[i] {
                if redoubt_rt::handle::device_info(*handle).is_err() {
                    // The manifest's name for it: its registers and its interrupt were placed
                    // under the server's name for it, the interrupt's with `-irq` added.
                    let named = |d: &&DeviceUse| {
                        *placed == d.name || placed.strip_suffix("-irq") == Some(d.name.as_str())
                    };
                    let device =
                        m.servers[i].devices.iter().find(named).map_or(placed.as_str(), |d| &d.device);
                    self.reboot(format_args!("{name}'s device {device} was quarantined"));
                }
            }
            let now = redoubt_rt::handle::time_now().unwrap_or(u64::MAX);
            if !self.restarts[i].restart(now) {
                self.reboot(format_args!(
                    "{name} was restarted {} times within {} seconds",
                    restarts::MOST,
                    restarts::WINDOW / 1_000_000
                ));
            }
            self.restarting = true;
            // The dead steward's carves outlive it under `users`: every principal's budgets, their
            // sessions and leases. They go before it starts again, so the new instance finds
            // `users` empty; a reap that fails is a restart `init` cannot make.
            if is_steward(m, &m.servers[i]) {
                self.empty_users();
            }
            self.start(i);
            self.restarting = false;
        }

        /// Reaps `users` to empty, one principal's subtree per kernel entry, the work billed to
        /// `init` (kernel/budgets.md, R10), and says how many went. Any budget holds its own page
        /// in its parent's usage, so a `users` whose usage is 0 has no children.
        fn empty_users(&mut self) {
            let users = Budget::from_handle(h(USERS_BUDGET));
            let occupied = users.usage().map(|u| u.pages_usage > 0 || u.processes_usage > 0);
            match occupied.and_then(|occupied| restarts::empty(occupied, || users.reap())) {
                Ok(n) => self.say(format_args!("init: emptied users: {n} budgets reaped")),
                Err(e) => self.reboot(format_args!("users cannot be emptied: {e:?}")),
            }
        }

        /// Step 3: asks `keyd` whether it holds any key the box is authenticated by; a yes refuses
        /// the boot (R35).
        fn check_keys(&mut self, i: usize) -> Result<(), Stop> {
            let plan = self.plan;
            for (at, key) in &plan.keys {
                let keyd = self.own(i)?;
                let holds = keyd::Message::Holds(keyd::Holds { key: &key[..] });
                let held = typed::call::<keyd::Protocol, _>(
                    &keyd,
                    &mut self.lend,
                    &holds,
                    &[],
                    |r, _| matches!(r, keyd::Reply::Holds(r) if r.held != 0),
                );
                match held {
                    Ok(false) => {}
                    Ok(true) => return Err(Stop::Failed(Refusal::KeyHeld { at: at.clone() })),
                    Err(e) => return Err(self.stop(i, e, "ask about a key")),
                }
            }
            self.say(format_args!("init: keyd holds none of the {} keys", plan.keys.len()));
            Ok(())
        }

        /// Step 4's first half: `init` stops writing to the UART and unmaps it, so `consoled` is
        /// the one process that maps it.
        fn give_up_the_uart(&mut self) {
            if let Out::Uart(registers) = core::mem::replace(&mut self.out, Out::Nowhere) {
                if registers.unmap().is_err() {
                    self.failed("consoled", "unmap the UART for");
                }
            }
        }

        /// Step 4's second half: `init`'s own connection to `consoled`, which its lines go
        /// through from now on and which each later child's console is minted from. A restarted
        /// `consoled` starts with empty tables, so every running server's console is gone.
        fn attach_console(&mut self, i: usize) -> Result<(), Stop> {
            let at = self.own(i)?;
            let ended = ENDED[i].load(Ordering::SeqCst);
            let console = Connection::attach(at, &mut self.lend).map_err(|e| self.stop(i, e, "attach to"))?;
            let file = console.open(&mut self.lend, "", mode::OWRITE).map_err(|e| self.stop(i, e, "open"))?;
            self.out = Out::Console(file);
            self.console = Some(console);
            self.console_at = ended;
            let name = &self.manifest.servers[i].name;
            if ended == 0 {
                self.milestone(format_args!("init: started {name}, and writes through it"));
            } else {
                self.say(format_args!(
                    "init: restarted {name}; every other server's console connection is gone until it restarts"
                ));
            }
            Ok(())
        }

        /// Step 5's last part: every `public` entry's bytes to `bootfsd` `i`, then `seal`.
        fn push_public(&mut self, i: usize) -> Result<(), Stop> {
            let m = self.manifest;
            for entry in &m.public {
                let data = self.entries.iter().find(|e| e.name == *entry).expect("the check found it").data;
                for (n, chunk) in data.chunks(CHUNK).enumerate() {
                    let bootfsd = self.own(i)?;
                    let add = bootfs::Add { name: entry.as_str(), offset: (n * CHUNK) as u64, data: chunk };
                    let add = bootfs::Message::Add(add);
                    typed::call::<bootfs::Protocol, _>(&bootfsd, &mut self.lend, &add, &[], |_, _| ())
                        .map_err(|e| self.stop(i, e, "add a public entry to"))?;
                }
            }
            let bootfsd = self.own(i)?;
            let seal = bootfs::Message::Seal(bootfs::Seal {});
            typed::call::<bootfs::Protocol, _>(&bootfsd, &mut self.lend, &seal, &[], |_, _| ())
                .map_err(|e| self.stop(i, e, "seal"))?;
            let name = &m.servers[i].name;
            self.say(format_args!("init: {} public entries pushed to {name}, and sealed", m.public.len()));
            Ok(())
        }
    }

    /// What a watching thread watches: a server, by index, and its exit endpoint.
    struct Watched {
        server: usize,
        exit: Handle,
    }

    impl Watched {
        /// Both in the one word `thread_create` passes. Each fits 16 bits: there are fewer servers
        /// than threads, and fewer handles than `MAX_HANDLES`.
        fn word(&self) -> usize { (self.server << 16) | self.exit.index() as usize }

        fn from_word(word: usize) -> Watched {
            Watched { server: word >> 16, exit: h((word & 0xffff) as u32) }
        }
    }

    /// A server's watching thread: `arg` is the [`Watched`] server, as a word. For each of its
    /// instances it waits for the exit notice, keeps its blame, counts the end, destroys the
    /// instance's budget, and reports the end on [`REPORTS`]. Only `init` holds the endpoint, so
    /// nothing else should arrive, and anything that does is refused.
    extern "C" fn watch(arg: usize) -> ! {
        let watched = Watched::from_word(arg);
        let i = watched.server;
        let exit = Endpoint::from_handle(watched.exit);
        let reports = Endpoint::from_handle(h(REPORTS.load(Ordering::Acquire)));
        loop {
            match exit.receive(FOREVER, 0) {
                Ok(Event::Exit(notice)) => {
                    let labels = notice.blamed_labels.as_slice();
                    keep(i, 0, notice.blamed_account);
                    keep(i, 1, labels.len() as u64);
                    for (n, label) in labels.iter().enumerate() {
                        keep(i, 2 + n, *label);
                    }
                    ENDED[i].fetch_add(1, Ordering::SeqCst);
                    // The budget ends what the instance left, and fails a call `init` queued at
                    // the server through its own handle, stamped with it. A killed instance's
                    // budget is gone already, and its slot may hold another handle by now.
                    if notice.cause != Cause::Killed {
                        let _ = Budget::from_handle(h(BUDGETS[i].load(Ordering::SeqCst))).destroy();
                    }
                    let words =
                        [i as u64, u64::from(notice.pid), notice.cause as u64, u64::from(notice.code)];
                    let _ = reports.send(&words, &[], None, FOREVER);
                }
                Ok(Event::Call(request)) => drop(request),
                Ok(Event::Send(delivery)) => close_delivery(&delivery),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        redoubt_rt::handle::thread_exit()
    }

    /// The boot, given the bundle the loader mapped. It never returns: the first thread ends
    /// restarting the servers, or powers the machine off, or reboots it.
    pub fn boot(initrd: &'static [u8]) -> u32 {
        // Root's free pages before the arena is taken, so the bound counts the arena once.
        let root = Budget::from_handle(h(ROOT)).usage();
        let arena = redoubt_rt::fix_heap(ARENA_PAGES);
        let Ok(registers) = Mmio::from_handle(h(CONSOLE_MMIO)).registers() else {
            redoubt_rt::handle::process_exit(1)
        };
        let out = Out::Uart(registers);
        let Ok(mut lend) = Lend::new(LEND_PAGES as usize) else {
            refuse(&out, None, &Refusal::failed("init", "map its lend"))
        };
        #[cfg(not(feature = "boot-stats"))]
        out.write(None, "init: up\n");
        #[cfg(feature = "boot-stats")]
        out.write(None, &format!("init: up{}\n", stamp()));
        let (Ok(root), Ok(())) = (root, arena) else {
            refuse(&out, Some(&mut lend), &Refusal::failed("init", "take its arena"))
        };
        let Some(entries) = Bundle::new(initrd).and_then(|b| b.after_init()) else {
            refuse(&out, Some(&mut lend), &Refusal::NoManifest)
        };
        let Some(manifest) = entries.iter().find(|e| e.name == MANIFEST).map(|e| e.data) else {
            refuse(&out, Some(&mut lend), &Refusal::NoManifest)
        };
        let devices = devices();
        let sizes: Vec<(&str, usize)> = entries.iter().map(|e| (e.name, e.data.len())).collect();
        let Ok(machine) = machine(&devices, &sizes, root) else {
            refuse(&out, Some(&mut lend), &Refusal::failed("init", "read system's usage"))
        };
        let manifest = read(manifest, ARENA_PAGES).unwrap_or_else(|why| refuse(&out, Some(&mut lend), &why));
        let plan = check(&manifest, &machine, redoubt_signing::DEV_PUBLIC_KEY)
            .unwrap_or_else(|why| refuse(&out, Some(&mut lend), &why));
        let servers = manifest.servers.len();
        let mut boot = Boot {
            manifest: &manifest,
            plan: &plan,
            entries: &entries,
            out,
            lend,
            endpoints: Vec::new(),
            own: Vec::new(),
            console: None,
            console_at: 0,
            started: (0..servers).map(|_| None).collect(),
            exits: vec![None; servers],
            restarts: vec![Restarts::default(); servers],
            restarting: false,
            keyd: running(&manifest, "keyd").expect("the check refused a manifest without keyd"),
            consoled: running(&manifest, "consoled"),
            bootfsd: running(&manifest, BOOTFSD),
            public: false,
            root_before: root.pages_usage,
            closed: Closed::default(),
        };
        boot.say(format_args!(
            "init: the manifest is checked: {servers} servers, bound {} pages",
            plan.bound
        ));
        boot.run()
    }
}

redoubt_rt::first_entry!(machine::boot);
