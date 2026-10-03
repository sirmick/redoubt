//! `init`, the program: the one process the loader starts (kernel/boot.md, "The loader loads only
//! the kernel and `init`"). It reads the boot manifest from the bundle, checks all of it against
//! the machine ([`redoubt_init::check`]), and then starts the servers in the order
//! servers/init.md, "Starting the servers", gives: their endpoints, `keyd` and the key-separation
//! check, `consoled` with the UART `init` gave up, the rest, and the `public` entries pushed to
//! `bootfsd`. A refusal, from the check or from any step after it, is one line and a power-off
//! reporting a system failure, so no boot runs half started.
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
    use alloc::vec::Vec;
    use core::fmt;
    use core::num::NonZeroU64;
    use core::sync::atomic::{AtomicU32, Ordering};

    use redoubt_client::file::{Connection, File};
    use redoubt_client::grants::RELEASE_TIMEOUT;
    use redoubt_client::launch::{Job, Launch};
    use redoubt_client::typed;
    use redoubt_init::bound::{LEND_PAGES, WATCH_STACK_PAGES};
    use redoubt_init::bundle::{Bundle, Entry};
    use redoubt_init::check::{BOOTFSD, MANIFEST, Machine, Plan, args};
    use redoubt_init::manifest::Manifest;
    use redoubt_init::refusal::Refusal;
    use redoubt_init::{ARENA_PAGES, check, read};
    use redoubt_rt::abi::{
        BudgetSpec, Call, Error, FOREVER, Handle, Labels, PAGE_SIZE, ResetKind, Return, Usage,
    };
    use redoubt_rt::client::Lend;
    use redoubt_rt::handle::{Budget, Endpoint, Mmio, Registers, Reset};
    use redoubt_rt::ipc::{Buffer, Event};
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
    const RESET: u32 = 4;
    const CONSOLE_MMIO: u32 = 5;
    /// The stack each child starts on, in pages (`redoubt_client::launch`'s default).
    const STACK_PAGES: usize = 16;
    /// The bytes of a public entry each `add` carries: a page, inside the lend with its name.
    const CHUNK: usize = PAGE_SIZE;

    /// The 16550's registers `init` writes before `consoled` starts: transmit, and the line status
    /// whose bit 5 says the transmitter can take a byte.
    const THR: usize = 0;
    const LSR: usize = 5;
    const LSR_THR_EMPTY: u8 = 1 << 5;

    /// The endpoint the watching threads report exits on, which the first thread receives on.
    static REPORTS: AtomicU32 = AtomicU32::new(0);

    fn h(index: u32) -> Handle { Handle::new(index).expect("a slot is at least 1") }

    /// Where `init`'s lines go: the UART until `consoled` starts, then `init`'s own connection to
    /// `consoled`; nowhere in between.
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
            match redoubt_sys::syscall(&Call::DeviceInfo { device: handle }) {
                Ok(Return::Device(info)) => out.push((handle, info)),
                _ => break,
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
            stack_pages: STACK_PAGES,
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

    /// One server's running instance, as its exit report needs it.
    struct Started {
        server: usize,
        /// Its console connection's id at `consoled`, disconnected when it exits.
        console: Option<u64>,
        /// Kept, never waited on: the server's watching thread receives its exit notice.
        _job: Job,
    }

    /// The boot in progress.
    struct Boot<'a> {
        manifest: &'a Manifest,
        plan: &'a Plan,
        entries: &'a [Entry<'static>],
        out: Out,
        lend: Lend,
        /// Each endpoint a server receives on, by name: `init` keeps the receive right.
        endpoints: Vec<(&'a str, Endpoint)>,
        /// `init`'s own handle at each server it calls, by server index ([`Plan::init_badges`]).
        own: Vec<(usize, Endpoint)>,
        /// `init`'s own connection to `consoled`, once it runs.
        console: Option<Connection>,
        started: Vec<Started>,
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
        fn say(&mut self, line: fmt::Arguments) { self.out.write(Some(&mut self.lend), &format!("{line}\n")) }

        /// A step after the checks failed: a bug in the bound or the checks. `at` names where.
        fn failed(&mut self, at: &str, step: &'static str) -> ! {
            refuse(&self.out, Some(&mut self.lend), &Refusal::failed(at, step))
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

        /// `init`'s own handle at server `i`, which [`Plan::init_badges`] names.
        fn own(&self, i: usize) -> Endpoint {
            Endpoint::from_handle(self.own.iter().find(|(s, _)| *s == i).expect("init calls it").1.handle())
        }

        /// Steps 2 to 5: the endpoints, `keyd` and the key check, `consoled`, the rest, and the
        /// public entries; then the exits.
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
            for &(i, badge) in &self.plan.init_badges {
                let badge = NonZeroU64::new(badge).expect("init's badge is at least 1");
                let at = Endpoint::from_handle(self.endpoint(&m.servers[i].receives[0]));
                let Ok(own) = at.mint(badge, None) else {
                    self.failed(&m.servers[i].name, "mint init's handle at")
                };
                self.own.push((i, own));
            }
            let Ok(reports) = Endpoint::create() else { self.failed("init", "create its reports endpoint") };
            REPORTS.store(reports.handle().index(), Ordering::Release);
            let keyd = running(m, "keyd").expect("the check refused a manifest without keyd");
            let consoled = running(m, "consoled");
            self.start(keyd);
            self.check_keys(keyd);
            if let Some(i) = consoled {
                self.give_up_the_uart();
                self.start(i);
                self.attach_console(i);
            }
            for i in (0..m.servers.len()).filter(|&i| i != keyd && Some(i) != consoled) {
                self.start(i);
            }
            if let Some(i) = running(m, BOOTFSD) {
                self.push_public(i);
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
            self.say(format_args!(
                "init: the boot is done: {} servers, root holds {used} pages for it, within the bound of {}",
                m.servers.len(),
                self.plan.bound
            ));
            self.watch(&reports)
        }

        /// Starts server `i` in a budget of its own, with its watching thread.
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
            let Ok(exit) = Endpoint::create() else { self.failed(&s.name, "create the exit endpoint of") };
            let exit_handle = exit.handle();
            let image = self.entries.iter().find(|e| e.name == s.program).expect("the check found it").data;
            let mut console = None;
            if let Some(parent) = &self.console {
                let Ok(minted) = parent.new_connection(&mut self.lend, "", 0) else {
                    self.failed(&s.name, "mint a console connection for")
                };
                console = Some(minted);
            }
            let mut handed = Vec::new();
            for item in &s.handed {
                let badge = NonZeroU64::new(item.badge).expect("the check refused badge 0");
                let Ok(minted) = Endpoint::from_handle(self.endpoint(&item.endpoint)).mint(badge, None)
                else {
                    self.failed(&s.name, "mint a handed badge for")
                };
                handed.push((item.endpoint.as_str(), minted.handle()));
            }
            let mut launch = Launch::new(STUB_BIN, image, budget, exit);
            for name in &s.receives {
                launch.handle(name, self.endpoint(name));
            }
            for (name, minted) in &handed {
                launch.handle(name, *minted);
            }
            for (name, device) in &self.plan.placements[i] {
                launch.handle(name, *device);
            }
            if let Some((conn, _)) = &console {
                launch.namespace("/dev/cons", conn.handle());
            }
            for arg in args(m, s) {
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
            let Ok(stack) = Buffer::new(WATCH_STACK_PAGES as usize) else {
                self.failed(&s.name, "map the watching thread's stack for")
            };
            let watched = Watched { server: i, exit: exit_handle };
            if redoubt_rt::handle::thread_create(watch, stack, watched.word()).is_err() {
                self.failed(&s.name, "start the watching thread for");
            }
            let console = console.map(|(_, id)| id);
            // `consoled` itself starts while `init` writes nowhere: `attach_console` says it.
            match console {
                Some(id) => self.say(format_args!("init: started {}, console {id}", s.name)),
                None if !matches!(self.out, Out::Nowhere) => {
                    self.say(format_args!("init: started {}", s.name))
                }
                None => {}
            }
            self.started.push(Started { server: i, console, _job: job });
        }

        /// Step 3: asks `keyd` whether it holds any key the box is authenticated by; a yes refuses
        /// the boot (R35).
        fn check_keys(&mut self, i: usize) {
            let keyd = self.own(i);
            let plan = self.plan;
            for (at, key) in &plan.keys {
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
                    Ok(true) => refuse(&self.out, Some(&mut self.lend), &Refusal::KeyHeld { at: at.clone() }),
                    Err(_) => self.failed(&self.manifest.servers[i].name, "ask about a key"),
                }
            }
            self.say(format_args!("init: keyd holds none of the {} keys", plan.keys.len()));
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
        /// through from now on and which each later child's console is minted from.
        fn attach_console(&mut self, i: usize) {
            let name = &self.manifest.servers[i].name;
            let Ok(console) = Connection::attach(self.own(i), &mut self.lend) else {
                self.failed(name, "attach to")
            };
            let Ok(file) = console.open(&mut self.lend, "", mode::OWRITE) else { self.failed(name, "open") };
            self.out = Out::Console(file);
            self.console = Some(console);
            self.say(format_args!("init: started {name}, and writes through it"));
        }

        /// Step 5's last part: every `public` entry's bytes to `bootfsd` `i`, then `seal`.
        fn push_public(&mut self, i: usize) {
            let bootfsd = self.own(i);
            let m = self.manifest;
            let name = &m.servers[i].name;
            for entry in &m.public {
                let data = self.entries.iter().find(|e| e.name == *entry).expect("the check found it").data;
                for (n, chunk) in data.chunks(CHUNK).enumerate() {
                    let add = bootfs::Add { name: entry.as_str(), offset: (n * CHUNK) as u64, data: chunk };
                    let add = bootfs::Message::Add(add);
                    if typed::call::<bootfs::Protocol, _>(&bootfsd, &mut self.lend, &add, &[], |_, _| ())
                        .is_err()
                    {
                        self.failed(name, "add a public entry to");
                    }
                }
            }
            let seal = bootfs::Message::Seal(bootfs::Seal {});
            if typed::call::<bootfs::Protocol, _>(&bootfsd, &mut self.lend, &seal, &[], |_, _| ()).is_err() {
                self.failed(name, "seal");
            }
            self.say(format_args!("init: {} public entries pushed to {name}, and sealed", m.public.len()));
        }

        /// What comes after the boot: each exit printed under the server's manifest name, and its
        /// console connection released. Nothing restarts yet.
        fn watch(&mut self, reports: &Endpoint) -> ! {
            let m = self.manifest;
            loop {
                let Ok(Event::Send(report)) = reports.receive(FOREVER, 0) else { continue };
                let [server, pid, cause, code] = report.words;
                let Some(at) = self.started.iter().position(|s| s.server as u64 == server) else { continue };
                let started = self.started.swap_remove(at);
                if let (Some(console), Some(id)) = (&self.console, started.console) {
                    let _ = console.disconnect(id, RELEASE_TIMEOUT);
                }
                let cause = match cause {
                    1 => "exited",
                    2 => "faulted",
                    _ => "was killed",
                };
                let name = &m.servers[started.server].name;
                self.say(format_args!("init: {name} (PID {pid}) {cause}, code {code}"));
            }
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

    /// A server's watching thread: `arg` is the [`Watched`] server, as a word. It
    /// waits for the one exit notice and reports it on [`REPORTS`]; only `init` holds the
    /// endpoint, so nothing else should arrive, and anything that does is refused.
    extern "C" fn watch(arg: usize) -> ! {
        let watched = Watched::from_word(arg);
        let exit = Endpoint::from_handle(watched.exit);
        let reports = Endpoint::from_handle(h(REPORTS.load(Ordering::Acquire)));
        loop {
            match exit.receive(FOREVER, 0) {
                Ok(Event::Exit(notice)) => {
                    let server = watched.server as u64;
                    let words = [server, u64::from(notice.pid), notice.cause as u64, u64::from(notice.code)];
                    let _ = reports.send(&words, &[], None, FOREVER);
                    break;
                }
                Ok(Event::Call(request)) => drop(request),
                Ok(Event::Send(delivery)) => {
                    for handle in delivery.handles.as_slice().iter().flatten() {
                        let _ = redoubt_rt::handle::close(*handle);
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        redoubt_rt::handle::thread_exit()
    }

    /// The boot, given the bundle the loader mapped. It never returns: the first thread ends
    /// watching the servers' exits, or powers the machine off.
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
        out.write(None, "init: up\n");
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
        let mut boot = Boot {
            manifest: &manifest,
            plan: &plan,
            entries: &entries,
            out,
            lend,
            endpoints: Vec::new(),
            own: Vec::new(),
            console: None,
            started: Vec::new(),
            root_before: root.pages_usage,
            closed: Closed::default(),
        };
        boot.say(format_args!(
            "init: the manifest is checked: {} servers, bound {} pages",
            manifest.servers.len(),
            plan.bound
        ));
        boot.run()
    }
}

redoubt_rt::first_entry!(machine::boot);
