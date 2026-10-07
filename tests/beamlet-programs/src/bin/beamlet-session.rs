//! `beamlet-session`: the tester of beamlet's natives' cases, in the steward's place
//! (docs/userland/beamlet.md, "Natives"). `init` hands the manifest's steward the `users` budget,
//! and this program carves it as the steward does (docs/servers/steward.md, "Fixed sub-budgets per
//! label set"): the principal's top budget with its account, under it a sub-budget per label set
//! with that set's labels, and from that the session's budget; then it launches `/boot/beamlet`
//! in it with what a session gets (its own `budget`, a fresh console connection at `/dev/cons`, a
//! fresh `bootfsd` connection at `/boot` and as `bootfsd`, the home volume at `/home/alice`, a
//! fresh system volume connection), and no more, unless the case asks: `keyd` by name for the
//! generated client's call, and for the serving case an endpoint as `service`. What it runs is
//! `/boot/beamlet-session.args`, since `init` passes a steward no arguments of the manifest's: each
//! `vm=MODULE:FUNCTION` word is one VM, run to its end before the next; `labels=N[,N]` is the label
//! set of the VMs after it; with `serve`, it also launches `/boot/beamlet-caller` with a badged
//! handle to the served endpoint. Then it parks, as a steward must not exit.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_client::console::Console;
use redoubt_client::file::Connection;
use redoubt_client::launch::{Job, Launch};
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend};
use redoubt_init_programs::park;
use redoubt_rt::abi::{BudgetSpec, Error as SysError, FOREVER, Handle, Labels};
use redoubt_rt::handle::{Budget, Endpoint};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

#[cfg(target_os = "none")]
static STUB: &[u8] = include_bytes!(env!("STUB_BIN"));
#[cfg(not(target_os = "none"))]
static STUB: &[u8] = b"stub";

/// The principal, as the cases' manifests name alice: her account and her top budget's limits.
const PRINCIPAL_ACCOUNT: u64 = 1001;
const PRINCIPAL_PAGES: u64 = 32_768;
const PRINCIPAL_PROCESSES: u32 = 8;
const PRINCIPAL_WEIGHT: u32 = 1000;
/// A budget object's own pages, the steward's `cost`.
const COST: u64 = 1;
/// A session's budget, as the steward's manifest sizes it, and the VM's stack.
const SESSION_PAGES: u64 = 10_880;
const SESSION_PROCESSES: u32 = 2;
const SESSION_WEIGHT: u32 = 100;
const SESSION_STACK_PAGES: usize = 17;
/// The home volume's path in a session's namespace; the cases' volume's root is the home.
const HOME: &str = "/home/alice";
/// The caller's budget: a small program, with an account of its own to show in its requests.
const CALLER_PAGES: u64 = 256;
const CALLER_WEIGHT: u32 = 10;
const CALLER_ACCOUNT: u64 = 2002;
/// The badge the caller's handle to the served endpoint carries.
const CALLER_BADGE: u64 = 9;

/// What a launched program is given: namespace entries, named handles, arguments, and its stack
/// and heap pages if not the defaults.
#[derive(Default)]
struct Child<'a> {
    namespace: Vec<(&'a str, Handle)>,
    handles: Vec<(&'a str, Handle)>,
    args: Vec<&'a str>,
    pages: Option<(usize, u32)>,
}

/// One VM to run: its module and function, and its session's label set.
struct Vm {
    module: String,
    function: String,
    labels: Vec<u64>,
}

struct Session {
    lend: Lend,
    console: Console,
    boot: Connection,
    home: Connection,
    system: Connection,
    users: Handle,
}

impl Session {
    fn say(&mut self, line: &str) {
        let line = format!("[beamlet-session] {line}\n");
        let mut rest = line.as_bytes();
        while let Ok(n) = self.console.write(&mut self.lend, rest) {
            if n == 0 || n >= rest.len() {
                return;
            }
            rest = &rest[n..];
        }
    }

    /// What to run, the whole of `/boot/beamlet-session.args`.
    fn read_args(&mut self) -> Result<String, Error> {
        let file = self.boot.open(&mut self.lend, "beamlet-session.args", mode::OREAD)?;
        let mut buf = alloc::vec![0; 1024];
        let n = file.read_at(&mut self.lend, 0, &mut buf)?;
        let _ = file.close(&mut self.lend);
        buf.truncate(n);
        String::from_utf8(buf).map_err(|_| Error::Unexpected)
    }

    /// A budget carved from `parent`.
    fn carve(
        &self,
        parent: &Budget,
        pages: u64,
        processes: u32,
        weight: u32,
        labels: &[u64],
        account: u64,
    ) -> Result<Budget, SysError> {
        let labels = Labels::from_slice(labels).map_err(|_| SysError::TooLarge)?;
        let spec = BudgetSpec { pages, processes, weight, labels, account, deadline: FOREVER };
        parent.create_child(&spec)
    }

    /// The principal's top budget under `users`, with her account.
    fn principal(&self) -> Result<Budget, SysError> {
        let users = Budget::from_handle(self.users);
        self.carve(&users, PRINCIPAL_PAGES, PRINCIPAL_PROCESSES, PRINCIPAL_WEIGHT, &[], PRINCIPAL_ACCOUNT)
    }

    /// One label set's fixed sub-budget: an equal share of the principal's top budget among its
    /// `sets` label sets, less a budget's own cost, with that set's labels. Adding a label is the
    /// steward's slot's power: a `system`-class caller's.
    fn sub(&self, principal: &Budget, sets: u32, labels: &[u64]) -> Result<Budget, SysError> {
        let pages = (PRINCIPAL_PAGES / u64::from(sets)).saturating_sub(COST);
        let (processes, weight) = (PRINCIPAL_PROCESSES / sets, PRINCIPAL_WEIGHT / sets);
        self.carve(principal, pages, processes, weight, labels, PRINCIPAL_ACCOUNT)
    }

    /// A session's budget from its label set's sub-budget: the set's exact labels.
    fn session(&self, sub: &Budget, labels: &[u64]) -> Result<Budget, SysError> {
        self.carve(sub, SESSION_PAGES, SESSION_PROCESSES, SESSION_WEIGHT, labels, PRINCIPAL_ACCOUNT)
    }

    /// A fresh connection to `server`, as the steward gives a session its own.
    fn fresh(&mut self, server: &Connection) -> Result<Handle, Error> {
        server.new_connection(&mut self.lend, "", 0).map(|(endpoint, _)| endpoint.handle())
    }

    /// Launches `/boot/NAME` in `budget` with what `child` gives it, and returns its job.
    fn launch(&mut self, name: &str, budget: &Budget, child: &Child<'_>) -> Result<Job, Error> {
        let file = self.boot.open(&mut self.lend, name, mode::OREAD)?;
        let len = file.stat(&mut self.lend)?.length as usize;
        let exit = Endpoint::create()?;
        let lend = &mut self.lend;
        let mut read = |at: usize, buf: &mut [u8]| -> Result<(), SysError> {
            let mut done = 0;
            while done < buf.len() {
                match file.read_at(lend, (at + done) as u64, &mut buf[done..]) {
                    Ok(0) | Err(_) => return Err(SysError::InvalidArgument),
                    Ok(n) => done += n,
                }
            }
            Ok(())
        };
        let mut launch = Launch::streamed(STUB, len, &mut read, Budget::from_handle(budget.handle()), exit);
        for (path, h) in &child.namespace {
            launch.namespace(path, *h);
        }
        for (n, h) in &child.handles {
            launch.handle(n, *h);
        }
        for arg in &child.args {
            launch.arg(arg);
        }
        if let Some((stack, heap)) = child.pages {
            launch.stack_pages(stack).heap_pages(heap);
        }
        launch.start().map_err(|failed| failed.error)
    }
}

/// The VMs the args name, each with the label set of the last `labels=` word before it.
fn vms(args: &str) -> Vec<Vm> {
    let mut labels: Vec<u64> = Vec::new();
    let mut vms = Vec::new();
    for word in args.split_whitespace() {
        if let Some(set) = word.strip_prefix("labels=") {
            labels = set.split(',').filter_map(|l| l.parse().ok()).collect();
        } else if let Some(spec) = word.strip_prefix("vm=") {
            let (module, function) = match spec.split_once(':') {
                Some((m, f)) => (m.into(), f.into()),
                None => (spec.into(), "start".into()),
            };
            vms.push(Vm { module, function, labels: labels.clone() });
        }
    }
    vms
}

fn run(startup: &Startup) -> u32 {
    let Ok(mut lend) = Lend::new(1) else { return 2 };
    let Ok(ns) = Namespace::from_startup(startup, &mut lend) else { return 3 };
    let Ok(console) = Console::open(&ns, &mut lend) else { return 3 };
    let (Some(users), Some(boot), Some(home), Some(system)) = (
        startup.handle("users"),
        startup.handle("bootfsd"),
        startup.handle("littlefsd:data"),
        startup.handle("erofsd:system"),
    ) else {
        return 4;
    };
    let attach = |h: Handle, lend: &mut Lend| Connection::attach(Endpoint::from_handle(h), lend);
    let (Ok(boot), Ok(home), Ok(system)) =
        (attach(boot, &mut lend), attach(home, &mut lend), attach(system, &mut lend))
    else {
        return 5;
    };
    let mut s = Session { lend, console, boot, home, system, users };
    let Ok(args) = s.read_args() else {
        s.say("FAIL: no /boot/beamlet-session.args");
        park();
    };
    let vms = vms(&args);
    let word = |w: &str| args.split_whitespace().any(|a| a == w);
    let (serving, keyd) = (word("serve"), word("keyd"));
    // The steward's carve at its start: the principal, then a sub-budget per label set.
    let Ok(principal) = s.principal() else {
        s.say("FAIL: no principal budget");
        park();
    };
    let mut sets: Vec<Vec<u64>> = Vec::new();
    for vm in &vms {
        if !sets.contains(&vm.labels) {
            sets.push(vm.labels.clone());
        }
    }
    let mut subs: Vec<(Vec<u64>, Budget)> = Vec::new();
    for labels in &sets {
        let Ok(sub) = s.sub(&principal, sets.len() as u32, labels) else {
            s.say(&format!("FAIL: no sub-budget for labels {labels:?}"));
            park();
        };
        subs.push((labels.clone(), sub));
    }
    for Vm { module, function, labels } in &vms {
        let Some((_, sub)) = subs.iter().find(|(l, _)| l == labels) else { park() };
        let Ok(budget) = s.session(sub, labels) else {
            s.say("FAIL: no session budget");
            park();
        };
        // The session's own connections, fresh from their servers, as the steward gives them.
        let (cons, boot, home, system) =
            (s.console.file().connection().clone(), s.boot.clone(), s.home.clone(), s.system.clone());
        let (Ok(console), Ok(boot), Ok(home), Ok(system)) =
            (s.fresh(&cons), s.fresh(&boot), s.fresh(&home), s.fresh(&system))
        else {
            s.say("FAIL: no fresh connections");
            park();
        };
        let service = if serving { Endpoint::create().ok() } else { None };
        let pages = format!("budget_pages={SESSION_PAGES}");
        let heap = (SESSION_PAGES - SESSION_STACK_PAGES as u64 - 1) as u32;
        let mut child = Child {
            namespace: alloc::vec![("/dev/cons", console), ("/boot", boot), (HOME, home)],
            handles: alloc::vec![("budget", budget.handle()), ("bootfsd", boot), ("erofsd:system", system)],
            args: alloc::vec![&pages, "endpoint=erofsd:system", module, function],
            pages: Some((SESSION_STACK_PAGES, heap)),
        };
        if keyd {
            if let Some(h) = startup.handle("keyd") {
                child.handles.push(("keyd", h));
            }
        }
        if let Some(service) = &service {
            child.handles.push(("service", service.handle()));
        }
        let job = s.launch("beamlet", &budget, &child);
        let Ok(mut job) = job else {
            s.say(&format!("FAIL: beamlet {module} did not start"));
            park();
        };
        s.say(&format!("started beamlet {module}:{function}, labels {labels:?}"));
        let caller = match &service {
            Some(service) => start_caller(&mut s, service),
            None => None,
        };
        match job.wait(FOREVER) {
            Ok(ended) => s.say(&format!(
                "beamlet {module}:{function} ended: {:?}, code {}",
                ended.notice.cause, ended.notice.code
            )),
            Err(e) => s.say(&format!("FAIL: beamlet's notice: {e:?}")),
        }
        if let Some((mut job, caller_budget)) = caller {
            match job.wait(FOREVER) {
                Ok(ended) => s.say(&format!(
                    "beamlet-caller ended: {:?}, code {}",
                    ended.notice.cause, ended.notice.code
                )),
                Err(e) => s.say(&format!("FAIL: the caller's notice: {e:?}")),
            }
            let _ = caller_budget.destroy();
        }
        let _ = budget.destroy();
    }
    s.say("done");
    park()
}

/// Launches `/boot/beamlet-caller` in a budget of its own under `users`, another principal's
/// stand-in, with a badged handle to `service`.
fn start_caller(s: &mut Session, service: &Endpoint) -> Option<(Job, Budget)> {
    let users = Budget::from_handle(s.users);
    let budget = s.carve(&users, CALLER_PAGES, 1, CALLER_WEIGHT, &[], CALLER_ACCOUNT).ok()?;
    let badged = service.mint(NonZeroU64::new(CALLER_BADGE)?, None).ok()?;
    let console = s.console.file().connection().clone();
    let console = s.fresh(&console).ok()?;
    let child = Child {
        namespace: alloc::vec![("/dev/cons", console)],
        handles: alloc::vec![("service", badged.handle())],
        ..Child::default()
    };
    let job = s.launch("beamlet-caller", &budget, &child);
    match job {
        Ok(job) => Some((job, budget)),
        Err(e) => {
            s.say(&format!("FAIL: beamlet-caller did not start: {e:?}"));
            None
        }
    }
}
