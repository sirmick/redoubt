//! The rig: the bundle's first program, standing in for WP-R3's `init`. It holds every device and
//! the boot budgets (INTERIM, kernel `device.rs`), and launches the real `netd` and `ipd`, then
//! each case's programs, through the loader stub exactly as PACKAGES.md's "Launching a process"
//! describes, with the startup blocks `init` will write.
//!
//! **Placement** (WP-R3's rule): every launched image is copied to [`IMAGE_AT`], the startup page
//! is at [`STARTUP_AT`] and the stack ends at [`STACK_TOP`], with the unmapped page below the
//! stack as its guard: nothing a launcher maps is inside the program link range
//! `0x1_0000..STUB_ENTRY`.
//!
//! **The device.** The rig finds the network card by its virtio device ID among the MMIO
//! handles the loader made (the DMA-capable ones are virtio-mmio's eight slots), and its interrupt
//! by the same slot's position: the loader emits other MMIO in device-tree order and other
//! interrupts ascending (BOOT.md), QEMU `virt` gives slot k interrupt k + 1, and its device tree
//! lists the slots from the highest address down. A wrong pairing shows at once: `netd` never
//! hears a frame, so no positive control passes.
//!
//! **Verdicts.** The rig prints `[net-rig] ok: ...` for what it checked itself (exit codes of the
//! programs whose success is the point, the victim's reports, the bucket count), and
//! `[net-rig] FAIL: ...` otherwise; an attacker's exit code is printed as information only. What
//! reached the network is judged by the bench's peers and capture.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Write};
use core::num::NonZeroU64;

use redoubt_ipd::scope::{Ports, Prefix, Rule, Scope};
use redoubt_net_client::{REPORT, code, event};
use redoubt_rt::abi::{
    BudgetSpec, Cause, Error, ExitNotice, FOREVER, Handle, Labels, MemFlags, PAGE_SIZE, ResetKind,
};
use redoubt_rt::client::Client;
use redoubt_rt::handle::{Budget, Endpoint, Irq, Mmio, Process, Registers, Reset};
use redoubt_rt::ipc::{Buffer, Event};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::StartupBuilder;
use redoubt_rt::wire::proto::{ipd, net_ctl};
use stub::STUB_ENTRY;

use crate::SELF_ARGS;

/// The stub (flat) and the programs (stripped ELFs), built by `build.rs`.
static STUB: &[u8] = include_bytes!(env!("NET_RIG_STUB"));
static NETD: &[u8] = include_bytes!(env!("NET_RIG_NETD"));
static IPD: &[u8] = include_bytes!(env!("NET_RIG_IPD"));
static CLIENT: &[u8] = include_bytes!(env!("NET_RIG_CLIENT"));

/// Where a launched program's image, startup page and stack go (WP-R3's placement rule).
pub const IMAGE_AT: usize = 0x4000_0000;
pub const STARTUP_AT: usize = 0x7FF0_0000;
pub const STACK_TOP: usize = 0x8000_0000;
const STACK_PAGES: usize = 16;
const _: () = assert!(IMAGE_AT >= STUB_ENTRY + 0x10_0000 && STARTUP_AT >= STUB_ENTRY + 0x10_0000);

/// The handles the kernel gives the first program (INTERIM: `tests/programs/src/rd.rs`).
const SYSTEM: u32 = 2;
const USERS: u32 = 3;
const RESET: u32 = 4;
const CONSOLE: u32 = 5;
const OTHER_DEVICES: u32 = 7;

/// QEMU `virt`'s virtio-mmio slots, and what identifies a network card in one.
const VIRTIO_SLOTS: usize = 8;
const MAGIC: usize = 0x000;
const VERSION: usize = 0x004;
const DEVICE_ID: usize = 0x008;
const NET_DEVICE: u32 = 1;

/// The badges `ipd`'s and `netd`'s arguments name (below 2^63, each once).
const INGRESS: u64 = 3;
const ROOT: u64 = 4;
const NETD_CLIENT: u64 = 5;

/// The listen port the rig's root may give out, and the one it probes the link with.
const LISTEN_PORT: u16 = 8000;
const PROBE_PORT: u16 = 9;
/// How long the rig waits for `ipd`'s link (µs).
const LINK_WAIT: u64 = 20_000_000;

/// The peers (the bench's `[[net.peer]]`s; `tests/d3-net-*.toml`).
const ECHO_PEER: [u8; 4] = [10, 0, 9, 100];
const OUTSIDE_TARGET: [u8; 4] = [10, 0, 9, 101];
const FORWARDED_SELF: [u8; 4] = [10, 0, 9, 102];
const OUTSIDE_CONTROL: [u8; 4] = [10, 0, 9, 110];
const TWIN_PEER: [u8; 4] = [10, 0, 9, 111];
/// In scope but no peer: slirp refuses the SYN at once.
const UNANSWERED: [u8; 4] = [10, 0, 9, 200];
const PEER_PORT: u16 = 7;

/// What a rig binary runs: one per case.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// `bench-virtio-legacy-off`: find the card, say its transport version, power off.
    Probe,
    /// `d3-net-tcp`: a client round-trips bytes through a peer; a listener echoes the bench's dial.
    Tcp,
    /// The bench's self-checks: the echo client connects to the peer twice.
    Twice,
    /// `d3-net-attacks`: every attack with its positive control, then the bucket cap.
    Attacks,
    /// `d3-net-self-unrefused`: the same, with `ipd` not told 10.0.9.102 is the box's own.
    Unrefused,
    /// `bench-net-peer`: an echo through the peer, and a connect nobody answers, which must end.
    Peer,
    /// `d3-net-pinned`: parked calls abandoned, a parked read ended by `ipd`'s deadline, and the
    /// echo still working.
    Pinned,
}

/// The console: the UART the kernel handed over, written a byte at a time.
struct Console(Registers);

impl Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            // 16550: wait for the transmit holding register (LSR bit 5), then write it.
            while self.0.read_u8(5).is_some_and(|lsr| lsr & 0x20 == 0) {}
            self.0.write_u8(0, byte);
        }
        Ok(())
    }
}

macro_rules! say {
    ($rig:expr, $($arg:tt)*) => {{ let _ = writeln!($rig.out, $($arg)*); }};
}

fn h(index: u32) -> Handle { Handle::new(index).expect("handle 0") }

/// A program the rig started: where its exit notice arrives.
struct Child {
    exit: Endpoint,
    _process: Process,
    budget: Budget,
}

impl Child {
    /// Its exit notice, if it has ended; `None` while it runs.
    fn ended(&self) -> Option<ExitNotice> {
        match self.exit.receive(0, 0) {
            Ok(Event::Exit(notice)) => Some(notice),
            _ => None,
        }
    }

    /// Waits at most `limit` µs for its exit notice; `None` if it did not end by then.
    fn wait(&self, limit: u64) -> Option<ExitNotice> {
        match self.exit.receive(limit, 0) {
            Ok(Event::Exit(notice)) => Some(notice),
            _ => None,
        }
    }
}

/// How long a program whose end rests on `ipd`'s deadlines (60 s for a `ctl` wait, 30 s for a
/// read) may take (µs).
const ENDS_WITHIN: u64 = 150_000_000;

/// How long a program the rig runs to its end may take (µs). One still running then is killed,
/// and the case goes on: an attack that hangs is judged, like any other, by what reached the
/// network.
const RUN_LIMIT: u64 = 10_000_000;

/// How often a wait for a program's report looks whether the program has ended instead (µs).
const LOOK_EVERY: u64 = 100_000;

fn describe(notice: Option<ExitNotice>) -> String {
    match notice {
        Some(n) if n.cause == Cause::Exited => format!("exited {}", n.code),
        Some(n) if n.cause == Cause::Faulted => format!("faulted {}", n.code),
        Some(n) => format!("ended ({:?}) {}", n.cause, n.code),
        None => String::from("no exit notice"),
    }
}

struct Rig {
    out: Console,
    ok: bool,
    /// The device handles: from `OTHER_DEVICES` up to the log endpoint's receive right, which
    /// the kernel installs last (INTERIM: `tests/programs/src/rd.rs`, `log_rx`).
    devices: core::ops::Range<u32>,
    /// Where the programs report (`redoubt_net_client::REPORT`); each gets its own badge.
    reports: Endpoint,
    /// Reports taken while waiting for another: (badge, event, value).
    backlog: Vec<(u64, u64, u64)>,
    next_badge: u64,
    next_account: u64,
    /// The rig's root connection to `ipd`, and a 9P client on it.
    root: Handle,
    nine: Option<Client>,
    netd: Option<Child>,
    ipd: Option<Child>,
    /// Programs that must outlive the rig's checks (the bucket holders).
    holders: Vec<Child>,
}

/// Runs `mode` and powers the machine off.
pub fn run(mode: Mode) -> u32 {
    let registers = Mmio::from_handle(h(CONSOLE)).registers().expect("the console's registers");
    let devices = OTHER_DEVICES..first_free() - 1;
    let reports = Endpoint::create().expect("the report endpoint");
    let mut rig = Rig {
        out: Console(registers),
        ok: true,
        devices,
        reports,
        backlog: Vec::new(),
        next_badge: 100,
        next_account: 100,
        root: h(1),
        nine: None,
        netd: None,
        ipd: None,
        holders: Vec::new(),
    };
    say!(rig, "\n[net-rig] starting: {mode:?}");
    if let Err(why) = rig.case(mode) {
        rig.fail(&why);
    }
    if rig.ok {
        say!(
            rig,
            "[net-rig] {} PASSED",
            match mode {
                Mode::Probe => "NET PROBE",
                Mode::Tcp | Mode::Twice => "D3 NET TCP",
                Mode::Peer => "NET PEER",
                Mode::Pinned => "D3 NET PINNED",
                Mode::Attacks | Mode::Unrefused => "D3 NET ATTACKS",
            }
        );
    }
    let _ = Reset::from_handle(h(RESET)).reset(ResetKind::PowerOff);
    0
}

impl Rig {
    fn fail(&mut self, why: &str) {
        self.ok = false;
        say!(self, "[net-rig] FAIL: {why}");
    }

    fn check(&mut self, passed: bool, what: &str) {
        if passed {
            say!(self, "[net-rig] ok: {what}");
        } else {
            self.fail(what);
        }
    }

    fn case(&mut self, mode: Mode) -> Result<(), String> {
        let (mmio, irq, version) = self.find_net()?;
        say!(self, "[net-rig] net device: virtio-mmio version {version}");
        if mode == Mode::Probe {
            return Ok(());
        }
        let selfs: &[&str] = if mode == Mode::Unrefused { &SELF_ARGS[..1] } else { SELF_ARGS };
        self.start_network(mmio, irq, selfs)?;
        match mode {
            Mode::Probe => {}
            Mode::Tcp => self.tcp(1)?,
            Mode::Twice => self.tcp(2)?,
            Mode::Attacks | Mode::Unrefused => self.attacks()?,
            Mode::Peer => self.peer()?,
            Mode::Pinned => self.pinned()?,
        }
        self.check_alive();
        Ok(())
    }

    // ---- the device ----

    /// The network card's MMIO and interrupt handles and its transport version.
    fn find_net(&mut self) -> Result<(Handle, Handle, u32), String> {
        let (mut virtio, mut irqs) = (Vec::new(), Vec::new());
        for index in self.devices.clone() {
            let handle = h(index);
            match Irq::from_handle(handle).wait(0) {
                Err(Error::Timeout) | Ok(()) => irqs.push(handle),
                _ => {
                    // Only virtio-mmio is DMA-capable (the loader's rule): one page tells.
                    let mmio = Mmio::from_handle(handle);
                    if let Ok((addr, _)) = mmio.dma_alloc(1) {
                        let _ = redoubt_rt::handle::unmap(addr, PAGE_SIZE);
                        virtio.push(handle);
                    }
                }
            }
        }
        if virtio.len() != VIRTIO_SLOTS || irqs.len() < VIRTIO_SLOTS {
            return Err(format!(
                "{} virtio-mmio slots and {} interrupts, not 8 and 8+",
                virtio.len(),
                irqs.len()
            ));
        }
        let mut found = None;
        for (position, handle) in virtio.iter().enumerate() {
            let (base, len) =
                Mmio::from_handle(*handle).map().map_err(|e| format!("mapping a slot: {e:?}"))?;
            let magic = read32(base, len, MAGIC);
            if magic != u32::from_le_bytes(*b"virt") {
                return Err(format!("slot {position}: magic {magic:#x}"));
            }
            if read32(base, len, DEVICE_ID) == NET_DEVICE {
                if found.is_some() {
                    return Err(String::from("two network cards"));
                }
                // Device-tree order lists the slots from the top down; interrupts go up.
                let slot = VIRTIO_SLOTS - 1 - position;
                found = Some((*handle, irqs[slot], read32(base, len, VERSION)));
            }
        }
        found.ok_or_else(|| String::from("no network card"))
    }

    // ---- launching ----

    /// Starts `image` through the stub in a new budget from `parent` with `spec`, giving it
    /// `handles` under their names and `args`.
    fn launch(
        &mut self,
        parent: u32,
        spec: &BudgetSpec,
        image: &[u8],
        handles: &[(&str, Handle)],
        args: &[&str],
    ) -> Result<Child, String> {
        let at = |step: &'static str| move |e: Error| format!("launch: {step}: {e:?}");
        let budget = Budget::from_handle(h(parent)).create_child(spec).map_err(at("budget"))?;
        let exit = Endpoint::create().map_err(at("exit endpoint"))?;
        let process = Process::create(&budget, &exit).map_err(at("process_create"))?;
        let rw = MemFlags::READ | MemFlags::WRITE;
        let place = |bytes: &[u8], dst: usize, flags: MemFlags| -> Result<(), Error> {
            let len = bytes.len().max(1).next_multiple_of(PAGE_SIZE);
            let scratch = redoubt_rt::handle::map_anon(len, rw)?;
            copy_in(scratch, bytes);
            process.map(scratch, dst, len, flags)
        };
        place(STUB, STUB_ENTRY, MemFlags::READ | MemFlags::EXECUTE).map_err(at("map the stub"))?;
        place(image, IMAGE_AT, rw).map_err(at("map the image"))?;
        let stack = STACK_PAGES * PAGE_SIZE;
        let scratch = redoubt_rt::handle::map_anon(stack, rw).map_err(at("stack"))?;
        process.map(scratch, STACK_TOP - stack, stack, rw).map_err(at("map the stack"))?;
        let mut block = StartupBuilder::new(handles.len() as u32);
        block.image(IMAGE_AT, image.len());
        for (slot, (name, _)) in handles.iter().enumerate() {
            block.handle(name, h(slot as u32 + 1));
        }
        for arg in args {
            block.arg(arg);
        }
        let block = block.finish().map_err(|e| format!("launch: startup block: {e:?}"))?;
        place(&block, STARTUP_AT, MemFlags::READ).map_err(at("map the startup page"))?;
        let list: Vec<Handle> = handles.iter().map(|(_, handle)| *handle).collect();
        process.start(STUB_ENTRY, STACK_TOP - 16, STARTUP_AT, &list).map_err(at("process_start"))?;
        Ok(Child { exit, _process: process, budget })
    }

    /// `netd`, then `ipd`, then waits until `ipd` has a link.
    fn start_network(&mut self, mmio: Handle, irq: Handle, selfs: &[&str]) -> Result<(), String> {
        let netd_ep = Endpoint::create().map_err(|e| format!("netd's endpoint: {e:?}"))?;
        let ipd_ep = Endpoint::create().map_err(|e| format!("ipd's endpoint: {e:?}"))?;
        let mint = |ep: &Endpoint, badge: u64| {
            ep.mint(NonZeroU64::new(badge).unwrap(), None)
                .map(|e| e.handle())
                .map_err(|e| format!("mint: {e:?}"))
        };
        let ingress = mint(&ipd_ep, INGRESS)?;
        let client = mint(&netd_ep, NETD_CLIENT)?;
        self.root = mint(&ipd_ep, ROOT)?;

        let system = |pages| BudgetSpec {
            pages,
            processes: 1,
            weight: 100,
            labels: Labels::new(),
            account: 0,
            deadline: FOREVER,
        };
        let client_arg = format!("client={NETD_CLIENT}");
        let netd_handles = [("netd", netd_ep.handle()), ("net", mmio), ("net-irq", irq), ("ipd", ingress)];
        self.netd = Some(self.launch(SYSTEM, &system(1024), NETD, &netd_handles, &[&client_arg])?);

        let mut args: Vec<String> = Vec::new();
        args.push(String::from("addr=10.0.2.15/24"));
        args.push(String::from("gateway=10.0.2.2"));
        for prefix in selfs {
            args.push(format!("self={prefix}"));
        }
        args.push(format!("ingress={INGRESS}"));
        args.push(format!(
            "scope={ROOT}:c:0.0.0.0/0:1-65535,l:{LISTEN_PORT},l:{PROBE_PORT},l:{}",
            redoubt_net_client::code::PIN_LISTEN_PORT
        ));
        args.push(String::from("buckets=4"));
        // The rig's root holds a grant for every program alive at once: the victim, the labelled
        // caller and three holders in the attack case, more than the default 4.
        args.push(format!("limits={ROOT}:5:8:8"));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let ipd_handles = [("ipd", ipd_ep.handle()), ("netd", client)];
        self.ipd = Some(self.launch(SYSTEM, &system(4096), IPD, &ipd_handles, &args)?);
        say!(self, "[net-rig] started netd and ipd (self={})", selfs.join(" self="));
        self.wait_for_link()
    }

    /// `listen` answers `unreachable` until `ipd` has asked `netd` for the MAC; the rig listens on
    /// its probe port until it does not, then closes that socket. Nothing goes on the wire.
    fn wait_for_link(&mut self) -> Result<(), String> {
        let mut nine =
            Client::new(Endpoint::from_handle(self.root), 1).map_err(|e| format!("root client: {e:?}"))?;
        nine.attach(0, "").map_err(|e| format!("root attach: {e:?}"))?;
        nine.walk(0, 1, "tcp/clone").map_err(|e| format!("root clone: {e:?}"))?;
        nine.open(1, mode::OREAD).map_err(|e| format!("root clone: {e:?}"))?;
        let mut n = [0u8; 4];
        nine.read(1, 0, &mut n).map_err(|e| format!("root clone read: {e:?}"))?;
        nine.clunk(1).map_err(|e| format!("root clunk: {e:?}"))?;
        let ctl = format!("tcp/{}/ctl", u32::from_le_bytes(n));
        nine.walk(0, 2, &ctl).map_err(|e| format!("root ctl: {e:?}"))?;
        nine.open(2, mode::ORDWR).map_err(|e| format!("root ctl: {e:?}"))?;
        let listen = encode(net_ctl::Message::Listen(net_ctl::Listen { port: PROBE_PORT, backlog: 1 }));
        let started = now();
        let mut tries = 0u32;
        loop {
            tries += 1;
            if nine.write(2, 0, &listen).is_ok() {
                break;
            }
            if now().saturating_sub(started) > LINK_WAIT {
                return Err(format!("ipd had no link after {tries} tries"));
            }
        }
        let close = encode(net_ctl::Message::Close(net_ctl::Close {}));
        nine.write(2, 0, &close).map_err(|e| format!("closing the probe: {e:?}"))?;
        nine.clunk(2).map_err(|e| format!("root clunk: {e:?}"))?;
        say!(self, "[net-rig] ipd has a link ({tries} probes)");
        self.nine = Some(nine);
        Ok(())
    }

    /// A connection to `ipd` scoped to `rules`, through a real `grant` on the rig's root: the
    /// handle and the id the rig disconnects it by.
    fn grant(&mut self, rules: &[Rule]) -> Result<(Handle, u64), String> {
        let scope = Scope::new(rules).map_err(|_| String::from("grant: a bad scope"))?.encode();
        let mut page = Buffer::new(1).map_err(|e| format!("grant: {e:?}"))?;
        let words = ipd::Message::Grant(ipd::Grant { scope: &scope })
            .encode(&mut page)
            .map_err(|e| format!("grant: {e:?}"))?;
        let (reply, page) = Endpoint::from_handle(self.root)
            .call(&words, &[], Some(page), FOREVER)
            .into_result()
            .map_err(|e| format!("grant: {e:?}"))?;
        let lend = page.as_deref().unwrap_or(&[]);
        let handles = reply.handles.as_slice();
        match (ipd::Reply::decode(16, &reply.words, lend, handles.len()), handles) {
            (Ok(Ok(ipd::Reply::Grant(g))), [Some(conn)]) => Ok((*conn, g.id)),
            (other, _) => Err(format!("grant refused: {other:?}")),
        }
    }

    fn disconnect(&mut self, id: u64) {
        let done = self.nine.as_mut().map(|nine| nine.disconnect(id));
        if !matches!(done, Some(Ok(()))) {
            self.fail(&format!("disconnect {id}: {done:?}"));
        }
    }

    /// Launches a `net-client` with `role` and `args` on a fresh grant of `rules`, in a new
    /// budget of a new account (labelled with `labels`). Returns it, the grant's id and the badge
    /// its reports arrive on.
    fn client(&mut self, rules: &[Rule], labels: &[u64], args: &[&str]) -> Result<(Child, u64, u64), String> {
        let (conn, id) = self.grant(rules)?;
        self.next_badge += 1;
        let badge = self.next_badge;
        let report =
            self.reports.mint(NonZeroU64::new(badge).unwrap(), None).map_err(|e| format!("mint: {e:?}"))?;
        self.next_account += 1;
        let spec = BudgetSpec {
            pages: 512,
            processes: 1,
            weight: 10,
            labels: Labels::from_slice(labels).map_err(|e| format!("labels: {e:?}"))?,
            account: self.next_account,
            deadline: FOREVER,
        };
        // Every client is user-class, under USERS, as a principal's program is; a labelled one too:
        // adding labels needs the *caller's* budget to be system-class (KERNEL-SPEC.md,
        // `budget_create` labels), and the rig runs in the root budget, which is.
        let child = self.launch(USERS, &spec, CLIENT, &[("net", conn), ("rig", report.handle())], args)?;
        // The child has its own copies now.
        let _ = redoubt_rt::handle::close(conn);
        let _ = report.close();
        Ok((child, id, badge))
    }

    /// Runs a client to its end and returns how it ended; then disconnects its grant.
    fn run_client(
        &mut self,
        rules: &[Rule],
        labels: &[u64],
        args: &[&str],
    ) -> Result<Option<ExitNotice>, String> {
        self.run_client_for(rules, labels, args, RUN_LIMIT)
    }

    /// [`Rig::run_client`], giving it `limit` µs rather than [`RUN_LIMIT`].
    fn run_client_for(
        &mut self,
        rules: &[Rule],
        labels: &[u64],
        args: &[&str],
        limit: u64,
    ) -> Result<Option<ExitNotice>, String> {
        let (child, id, _) = self.client(rules, labels, args)?;
        let notice = child.wait(limit);
        if notice.is_none() {
            say!(self, "[net-rig] {} did not end within {} s: killed", args.join(" "), limit / 1_000_000);
        }
        self.disconnect(id);
        // Its weight and pages go back to the parent for the next.
        if let Err(e) = child.budget.destroy() {
            self.fail(&format!("destroying a finished program's budget: {e:?}"));
        }
        Ok(notice)
    }

    /// Waits for report `what` from `badge`, sent by `from`, and returns its value, answering
    /// every report. A program that ends first fails the wait, saying how it ended.
    fn report(&mut self, from: &Child, badge: u64, what: u64) -> Result<u64, String> {
        loop {
            if let Some(i) = self.backlog.iter().position(|(b, w, _)| *b == badge && *w == what) {
                return Ok(self.backlog.remove(i).2);
            }
            match self.take_report(LOOK_EVERY) {
                Some(report) => self.backlog.push(report),
                None => {
                    if let Some(notice) = from.ended() {
                        return Err(format!(
                            "a program ended before its report {what}: {}",
                            describe(Some(notice))
                        ));
                    }
                }
            }
        }
    }

    /// One report, answered, or `None` if none came within `timeout`.
    fn take_report(&mut self, timeout: u64) -> Option<(u64, u64, u64)> {
        loop {
            match self.reports.receive(timeout, 0) {
                Ok(Event::Call(request)) => {
                    let words = request.words;
                    let badge = request.caller.badge;
                    let _ = request.reply(&[0; 4], &[]);
                    if words[0] == REPORT {
                        return Some((badge, words[1], words[2]));
                    }
                }
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    }

    fn check_alive(&mut self) {
        let netd = self.netd.as_ref().and_then(Child::ended);
        let ipd = self.ipd.as_ref().and_then(Child::ended);
        let still = netd.is_none() && ipd.is_none();
        let what = format!("netd and ipd still run (netd: {}, ipd: {})", alive(netd), alive(ipd));
        self.check(still, &what);
    }

    // ---- the cases ----

    /// A client round-trips bytes through the echo peer `rounds` times; a listener with a backlog
    /// of 2 echoes the bench's dial.
    fn tcp(&mut self, rounds: u32) -> Result<(), String> {
        let echo = [Rule::Connect(prefix(ECHO_PEER, 32), ports(PEER_PORT))];
        let times = format!("times={rounds}");
        let notice = self.run_client(&echo, &[], &["role=echo", "addr=10.0.9.100", "port=7", &times])?;
        let passed = matches!(notice, Some(n) if n.cause == Cause::Exited && n.code == 0);
        self.check(passed, &format!("echo through 10.0.9.100:7, {rounds} round(s): {}", describe(notice)));

        let listen = [Rule::Listen(ports(LISTEN_PORT))];
        let (listener, _, badge) = self.client(&listen, &[], &["role=listen", "port=8000", "backlog=2"])?;
        self.report(&listener, badge, event::READY)?;
        say!(self, "[net-rig] listening on 8000 with a backlog of 2");
        let accepted = self.report(&listener, badge, event::ACCEPTED)?;
        self.check(accepted == 1, &format!("the listener accepted and echoed the bench's dial ({accepted})"));
        Ok(())
    }

    /// The bench's peer, both ways: the echo peer counts one connection, and a connect in scope to
    /// an address with no peer ends closed: slirp (`restrict=on`) refuses it at once with an RST
    /// (QA D3-code-review-final: this is a refusal, not a timeout; `d3-net-pinned` tests the
    /// deadlines).
    fn peer(&mut self) -> Result<(), String> {
        let echo = [Rule::Connect(prefix(ECHO_PEER, 32), ports(PEER_PORT))];
        let notice = self.run_client(&echo, &[], &["role=echo", "addr=10.0.9.100", "port=7"])?;
        let passed = matches!(notice, Some(n) if n.cause == Cause::Exited && n.code == 0);
        self.check(passed, &format!("echo through 10.0.9.100:7: {}", describe(notice)));
        let nowhere = [Rule::Connect(prefix(UNANSWERED, 32), ports(PEER_PORT))];
        let args = ["role=connect", "addr=10.0.9.200", "port=7"];
        let notice = self.run_client(&nowhere, &[], &args)?;
        let refused = matches!(notice, Some(n) if n.cause == Cause::Exited && n.code == code::CONNECTED + 4);
        self.check(refused, &format!("the connect slirp refuses ended closed: {}", describe(notice)));
        Ok(())
    }

    /// Pinned (plan 6.5): 64 parked reads given up by their caller, one parked read ended by
    /// `ipd`'s 30 s data deadline, the echo, then a listener's `ctl` read ended by `ipd`'s 60 s
    /// `ctl` deadline: the client exits 0 only if every step held.
    fn pinned(&mut self) -> Result<(), String> {
        let echo = [
            Rule::Connect(prefix(ECHO_PEER, 32), ports(PEER_PORT)),
            Rule::Listen(ports(redoubt_net_client::code::PIN_LISTEN_PORT)),
        ];
        let args = ["role=pin", "addr=10.0.9.100", "port=7", "times=64"];
        let notice = self.run_client_for(&echo, &[], &args, ENDS_WITHIN)?;
        let passed = matches!(notice, Some(n) if n.cause == Cause::Exited && n.code == 0);
        let what = format!(
            "64 abandoned reads, a read and an accept ended by ipd's deadlines, the echo: {}",
            describe(notice)
        );
        self.check(passed, &what);
        Ok(())
    }

    /// Every attack with its positive control in the same boot, each from its own account, then
    /// the bucket cap.
    fn attacks(&mut self) -> Result<(), String> {
        // The victim: listening on 8000 for the one connection it should get, the bench's dial.
        let listen = [Rule::Listen(ports(LISTEN_PORT))];
        let (victim, _, victim_badge) =
            self.client(&listen, &[], &["role=listen", "port=8000", "backlog=1"])?;
        self.report(&victim, victim_badge, event::READY)?;
        say!(self, "[net-rig] victim listening on 8000");

        // Outside its prefix: scoped to 10.0.9.110/32 port 7, it tries 10.0.9.101:7 (the peer must
        // count 0), then its control, 10.0.9.110:7 (the peer must count 1).
        let narrow = [Rule::Connect(prefix(OUTSIDE_CONTROL, 32), ports(PEER_PORT))];
        let attack = self.run_client(&narrow, &[], &["role=connect", "addr=10.0.9.101", "port=7"])?;
        say!(
            self,
            "[net-rig] attack outside the prefix to {}:7: {}",
            dotted(OUTSIDE_TARGET),
            describe(attack)
        );
        let control = self.run_client(&narrow, &[], &["role=connect", "addr=10.0.9.110", "port=7"])?;
        say!(self, "[net-rig] its control to {}:7: {}", dotted(OUTSIDE_CONTROL), describe(control));

        // The box's own addresses, from a scope that allows everything: a forwarded self address
        // (the peer must count 0), ipd's own address and loopback (the victim must see none, and
        // the capture no SYN), and the gateway, where the forwarded ports are.
        let any = [Rule::Connect(prefix([0, 0, 0, 0], 0), Ports::new(1, 65535).unwrap())];
        for (addr, port) in [
            (FORWARDED_SELF, PEER_PORT),
            ([10, 0, 2, 15], LISTEN_PORT),
            ([127, 0, 0, 1], LISTEN_PORT),
            ([127, 1, 2, 3], LISTEN_PORT),
            ([10, 0, 2, 2], LISTEN_PORT),
            ([10, 0, 2, 2], 22),
        ] {
            let (a, p) = (format!("addr={}", dotted(addr)), format!("port={port}"));
            let attack = self.run_client(&any, &[], &["role=connect", &a, &p])?;
            say!(self, "[net-rig] attack on the box's own {}:{port}: {}", dotted(addr), describe(attack));
        }
        // Its unlabelled twin, the same scope: its own peer is reachable (the peer must count 1).
        let twin = self.run_client(&any, &[], &["role=connect", "addr=10.0.9.111", "port=7"])?;
        say!(self, "[net-rig] the twin to {}:7: {}", dotted(TWIN_PEER), describe(twin));

        // A labelled caller with a wide scope, user-class like any principal's program: refused on
        // every path (its real connect attempt's peer must count 0), and
        // holding no bucket (below).
        // It stays, holding whatever it was given, until the buckets are counted.
        let (labelled, _, badge) = self.client(&any, &[7], &["role=labelled"])?;
        // What it says it got through is the attacker's own claim, printed for information only:
        // the verdicts are the bucket count below and the bench's count of its peer.
        let opened = self.report(&labelled, badge, event::LABELLED)?;
        say!(self, "[net-rig] the labelled caller reports (information only): not refused {opened:#x}");
        self.holders.push(labelled);

        // The victim got exactly the bench's dial.
        let accepted = self.report(&victim, victim_badge, event::ACCEPTED)?;
        self.check(accepted == 1, &format!("the victim accepted the bench's dial ({accepted})"));
        let more = self.take_report(0);
        self.check(
            more.is_none() && victim.ended().is_none(),
            "the victim accepted nothing more and still runs",
        );

        // Buckets: 4. The rig's root holds one (the victim's grant), the victim one; two more
        // accounts attach and hold, and a third is refused. Had the labelled caller (still
        // running) or any finished attacker kept a slot, the second holder would have been
        // refused.
        let held: Vec<bool> = (0..3)
            .map(|_| {
                let (holder, _, badge) = self.client(&any, &[], &["role=hold"])?;
                let attached = self.report(&holder, badge, event::ATTACH)? == 0;
                self.holders.push(holder);
                Ok(attached)
            })
            .collect::<Result<_, String>>()?;
        self.check(
            held == [true, true, false],
            &format!("buckets: two more accounts held, a third refused ({held:?})"),
        );
        Ok(())
    }
}

fn alive(notice: Option<ExitNotice>) -> String {
    match notice {
        None => String::from("running"),
        some => describe(some),
    }
}

fn prefix(addr: [u8; 4], len: u8) -> Prefix {
    Prefix::new(u32::from_be_bytes(addr), len).expect("a canonical prefix")
}

fn ports(port: u16) -> Ports { Ports::new(port, port).expect("a port") }

fn dotted(a: [u8; 4]) -> String { format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]) }

fn encode(message: net_ctl::Message<'_>) -> Vec<u8> {
    let mut out = alloc::vec![0u8; 16];
    let n = message.encode_file(&mut out).unwrap_or(0);
    out.truncate(n);
    out
}

fn now() -> u64 { redoubt_rt::handle::time_now().unwrap_or(u64::MAX) }

/// The lowest handle index this process does not hold: `budget_usage` answers `BadHandle` only
/// for an index that holds nothing.
fn first_free() -> u32 {
    (1..=redoubt_rt::abi::MAX_HANDLES as u32 + 1)
        .find(|i| Budget::from_handle(h(*i)).usage() == Err(Error::BadHandle))
        .expect("a free index")
}

/// A 32-bit register of a mapped virtio-mmio slot (QEMU refuses narrower reads of these).
fn read32(base: usize, len: usize, offset: usize) -> u32 {
    assert!(offset + 4 <= len, "a register outside the mapping");
    // SAFETY: `base..base + len` is the device mapping `map_device` made for this process, and
    // it stays mapped for the life of the process; `offset + 4 <= len` is checked above, and
    // `offset` is a multiple of 4 on a page-aligned base.
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

fn copy_in(dst: usize, src: &[u8]) {
    // SAFETY: `dst` is the start of pages this process just mapped read-write with `map_anon`,
    // at least `src.len()` bytes of them, and nothing else refers to them yet.
    let to = unsafe { core::slice::from_raw_parts_mut(dst as *mut u8, src.len()) };
    to.copy_from_slice(src);
}
