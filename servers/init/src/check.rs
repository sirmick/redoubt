//! The manifest checked whole, before anything runs (servers/init.md, "Starting the servers",
//! step 1). [`check`] takes the decoded manifest and what `init` learned from the machine, and
//! returns either the one refusal that stops the boot or the [`Plan`] the boot follows. It is
//! pure: the machine's answers come in as values, so host tests and the fuzz target drive the
//! same code the boot runs.

use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::net::IpAddr;

use redoubt_rt::abi::{Handle, MAX_LABELS, MAX_START_HANDLES, MAX_THREADS, Usage};
use redoubt_rt::server::minted::FIRST_MINTED_BADGE;
use redoubt_rt::startup::{StartupBuilder, valid_name};
use redoubt_sys::DeviceInfo;

use crate::bound::{self, Counts};
use crate::confine;
use crate::manifest::{Budget, Manifest, Server};
use crate::refusal::{Refusal, Why};
use crate::sshkey::{self, KEY_LEN};

/// The bundle entry that is the manifest. It is never public.
pub const MANIFEST: &str = "manifest";
/// The budgets `init` holds. No endpoint takes their names, and a server is handed none
/// (R33 (no server holds a system budget)).
pub const BUDGETS: [&str; 3] = ["root", "system", "users"];
/// The longest device name, so that `NAME-irq` is still a name (servers/init.md, "One entry per
/// device").
pub const MAX_DEVICE_NAME: usize = 60;
/// The suffix of a device's interrupt handle.
pub const IRQ_SUFFIX: &str = "-irq";
/// The argument that sizes a shared server (servers/serving.md R26).
pub const BUCKETS_ARG: &str = "buckets=";
/// The program that serves `/boot`: `init` pushes the `public` entries to it.
pub const BOOTFSD: &str = "bootfsd";
/// The programs `init` calls itself, each through a root badge of its own there: `keyd` for
/// `holds`, `consoled` for its own lines, `bootfsd` for the public entries.
pub const INIT_CALLS: [&str; 3] = ["keyd", "consoled", BOOTFSD];
/// The program that serves the disk's ranges: `init` mints each volume's range at it.
pub const BLKD: &str = "blkd";
/// The startup-block name of a volume's range, handed to the server attaching it.
pub const VOLUME: &str = "volume";
/// The argument giving a volume's server its label ids (servers/fsd.md, "Volumes, connections
/// and labels").
pub const LABELS_ARG: &str = "labels=";
/// The prefix of the arguments giving `blkd` each labelled range's ids, `labels.P=ID,...` for
/// GPT entry P (servers/blkd.md, "Ranges and badges").
pub const RANGE_LABELS_ARG: &str = "labels.";
/// Where [`Plan::keys`] says the bundle's verifying key comes from.
pub const BUNDLE_KEY: &str = "bundle key";

/// What `init` learned from the machine before it checks: everything [`check`] compares the
/// manifest with.
#[derive(Clone, Copy, Debug)]
pub struct Machine<'a> {
    /// Every device handle `init` holds, with `device_info`'s answer for it.
    pub devices: &'a [(Handle, DeviceInfo)],
    /// `system`'s limits and usage (`budget_usage`).
    pub system: Usage,
    /// `root`'s limits and usage: its free pages are what `init` may still spend.
    pub root: Usage,
    /// The bundle's entries after `init`: each name and its length in bytes.
    pub entries: &'a [(&'a str, usize)],
    /// The stub's length in bytes, and the stack each launch gives, in pages.
    pub stub_bytes: usize,
    pub stack_pages: usize,
    /// The arena `init` parses and checks in, in pages.
    pub arena_pages: usize,
    /// The handles in `init`'s table at its start.
    pub handles_at_start: usize,
}

/// The boot a manifest that passed every check asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// For each server, in manifest order, its devices' handles under the names it looks them up
    /// by.
    pub placements: Vec<Vec<(String, Handle)>>,
    /// The public keys `init` asks `keyd` about, each with where the manifest lists it: every
    /// login and approval key, then `bundle_key` as [`BUNDLE_KEY`] (R35 (key separation)).
    pub keys: Vec<(String, [u8; KEY_LEN])>,
    /// For each shared server, by index, the domains declared there: its `buckets=N` is at least
    /// this.
    pub buckets: Vec<(usize, u32)>,
    /// For each server `init` calls itself ([`INIT_CALLS`]), by index, the badge of `init`'s own
    /// handle at the first endpoint it receives on: the smallest from 1 that no `handed` item
    /// there uses, so `init` is a root caller with a share of its own (servers/serving.md R27).
    pub init_badges: Vec<(usize, u64)>,
    /// The bound on what the boot costs `init` in `root`, in pages.
    pub bound: u64,
}

fn at(at: String, why: Why) -> Refusal { Refusal::At { at, why } }

/// Checks `manifest` against `machine` and returns the plan, or the refusal that stops the boot.
/// The checks run in a fixed order, so a manifest that breaks several rules is refused for the
/// first: names, references, the servers `init` calls, devices, budgets and the fit in `system`,
/// `public`, the startup blocks, keys, buckets, confinement, and last the bound on `root`: the
/// threads that watch the servers, then the pages.
pub fn check(m: &Manifest, machine: &Machine, bundle_key: [u8; KEY_LEN]) -> Result<Plan, Refusal> {
    names(m)?;
    references(m, machine)?;
    init_calls(m)?;
    volumes(m)?;
    let placements = devices(m, machine)?;
    budgets(m)?;
    fit(m, &machine.system)?;
    public(m, machine)?;
    blocks(m, machine)?;
    let mut keys = keys(m)?;
    keys.push((String::from(BUNDLE_KEY), bundle_key));
    let buckets = buckets(m)?;
    if m.confined {
        confine::check(m)?;
    }
    // One thread per server watches its exit endpoint, beside `init`'s own.
    let most = MAX_THREADS - 1;
    if m.servers.len() > most {
        return Err(Refusal::Watchers { servers: m.servers.len(), most });
    }
    let bound = bound::bound(&counts(m, machine));
    let free = machine.root.pages_limit.saturating_sub(machine.root.pages_usage);
    if bound > free {
        return Err(Refusal::Bound { need: bound, free });
    }
    Ok(Plan { placements, keys, buckets, init_badges: init_badges(m), bound })
}

/// Every name follows the rule, and names of one kind differ.
fn names(m: &Manifest) -> Result<(), Refusal> {
    let mut seen = Unique::default();
    for (i, d) in m.devices.iter().enumerate() {
        device_name(&d.name, format!("devices[{i}].name"))?;
        seen.add(&d.name, "devices", || format!("devices[{i}].name"))?;
    }
    for (i, l) in m.labels.iter().enumerate() {
        name(&l.name, || format!("labels[{i}].name"))?;
        seen.add(&l.name, "labels", || format!("labels[{i}].name"))?;
    }
    let ids: Vec<u64> = m.labels.iter().map(|l| l.id).collect();
    if let Some(i) = (0..ids.len()).find(|i| ids[..*i].contains(&ids[*i])) {
        return Err(at(format!("labels[{i}].id"), Why::Twice));
    }
    for (i, v) in m.volumes.iter().enumerate() {
        name(&v.name, || format!("volumes[{i}].name"))?;
        seen.add(&v.name, "volumes", || format!("volumes[{i}].name"))?;
    }
    for (i, p) in m.principals.iter().enumerate() {
        name(&p.name, || format!("principals[{i}].name"))?;
        seen.add(&p.name, "principals", || format!("principals[{i}].name"))?;
    }
    for (i, s) in m.servers.iter().enumerate() {
        name(&s.name, || format!("servers[{i}].name"))?;
        seen.add(&s.name, "servers", || format!("servers[{i}].name"))?;
        name(&s.program, || format!("servers[{i}].program"))?;
        server_names(s, i)?;
        for (k, e) in s.receives.iter().enumerate() {
            seen.add(e, "endpoints", || format!("servers[{i}].receives[{k}]"))
                .map_err(|_| at(format!("servers[{i}].receives[{k}]"), Why::ReceivedTwice))?;
        }
    }
    Ok(())
}

/// The names one server's startup block will hold: its endpoints, the endpoints it is handed and
/// its devices' names, each a name, none a budget's, and all different, as the block requires.
fn server_names(s: &Server, i: usize) -> Result<(), Refusal> {
    let mut block = Unique::default();
    for (k, e) in s.receives.iter().enumerate() {
        let path = || format!("servers[{i}].receives[{k}]");
        endpoint_name(e, path)?;
        block.add(e, "", path)?;
    }
    for (k, h) in s.handed.iter().enumerate() {
        let path = || format!("servers[{i}].handed[{k}].endpoint");
        endpoint_name(&h.endpoint, path)?;
        block.add(&h.endpoint, "", path)?;
    }
    for (k, d) in s.devices.iter().enumerate() {
        let path = || format!("servers[{i}].devices[{k}].as");
        device_name(&d.name, path())?;
        block.add(&d.name, "", path)?;
        block.add(&format!("{}{IRQ_SUFFIX}", d.name), "", path)?;
    }
    Ok(())
}

/// An endpoint name: a name, and never a budget's (R33).
fn endpoint_name(e: &str, path: impl Fn() -> String) -> Result<(), Refusal> {
    name(e, &path)?;
    if BUDGETS.contains(&e) {
        return Err(at(path(), Why::BudgetHandle));
    }
    Ok(())
}

fn name(n: &str, path: impl Fn() -> String) -> Result<(), Refusal> {
    if valid_name(n) { Ok(()) } else { Err(at(path(), Why::NotAName)) }
}

fn device_name(n: &str, path: String) -> Result<(), Refusal> {
    if !valid_name(n) {
        return Err(at(path, Why::NotAName));
    }
    if n.len() > MAX_DEVICE_NAME || n.ends_with(IRQ_SUFFIX) {
        return Err(at(path, Why::DeviceName));
    }
    Ok(())
}

/// Names of each kind so far, to refuse one given twice.
#[derive(Default)]
struct Unique(BTreeSet<(&'static str, String)>);

impl Unique {
    fn add(&mut self, name: &str, kind: &'static str, path: impl Fn() -> String) -> Result<(), Refusal> {
        if self.0.insert((kind, String::from(name))) { Ok(()) } else { Err(at(path(), Why::Twice)) }
    }
}

/// The servers `init` calls itself ([`INIT_CALLS`]) each receive on an endpoint, the first of
/// which `init` calls; each runs once, since a second would run beside the one `init` calls and
/// go unchecked (a second `keyd` would hold keys `init` never asked about, R35); there is a
/// `keyd`, since the bundle key is always asked about; and no server is handed an endpoint a
/// `consoled` receives on, since a root badge there writes bare lines, which only `init`'s may be
/// (servers/consoled.md, "Started by `init`").
fn init_calls(m: &Manifest) -> Result<(), Refusal> {
    let called = |s: &&Server| INIT_CALLS.contains(&s.program.as_str());
    if let Some(i) = m.servers.iter().position(|s| called(&s) && s.receives.is_empty()) {
        return Err(at(format!("servers[{i}].receives"), Why::Unknown));
    }
    for (i, s) in m.servers.iter().enumerate() {
        let Some(&program) = INIT_CALLS.iter().find(|p| **p == s.program) else { continue };
        if m.servers[..i].iter().any(|t| t.program == program) {
            return Err(at(format!("servers[{i}].program"), Why::Second(program)));
        }
    }
    let consoled =
        |e: &str| m.servers.iter().any(|s| s.program == "consoled" && s.receives.iter().any(|r| r == e));
    for (i, s) in m.servers.iter().enumerate() {
        if let Some(k) = s.handed.iter().position(|h| consoled(&h.endpoint)) {
            return Err(at(format!("servers[{i}].handed[{k}].endpoint"), Why::ConsoledRoot));
        }
    }
    if !m.servers.iter().any(|s| s.program == "keyd") {
        return Err(at(String::from("servers"), Why::NoKeyd));
    }
    Ok(())
}

/// The `blkd` a volume's range is minted at, when there is exactly one.
pub fn blkd(m: &Manifest) -> Option<&Server> {
    let mut found = m.servers.iter().filter(|s| s.program == BLKD);
    found.next().filter(|_| found.next().is_none())
}

/// Each volume is one GPT entry served by at most one server (R47 (one volume per instance)),
/// and a volume a server attaches has one `blkd`, receiving on an endpoint, to mint its range at.
/// No entry is handed a badge where a `blkd` receives, since `blkd` resolves each badge there to
/// a volume's range, and no entry carries an argument `init` passes itself: `labels=` on a
/// volume's server, `labels.` on `blkd`.
fn volumes(m: &Manifest) -> Result<(), Refusal> {
    for (i, v) in m.volumes.iter().enumerate() {
        if m.volumes[..i].iter().any(|w| w.partition == v.partition) {
            return Err(at(format!("volumes[{i}].partition"), Why::Twice));
        }
    }
    for (i, s) in m.servers.iter().enumerate() {
        let Some(volume) = &s.volume else { continue };
        if m.servers[..i].iter().any(|t| t.volume.as_ref() == Some(volume)) {
            return Err(at(format!("servers[{i}].volume"), Why::Twice));
        }
        if blkd(m).is_none_or(|b| b.receives.is_empty()) {
            return Err(at(format!("servers[{i}].volume"), Why::NoBlkd));
        }
    }
    let ranges = |e: &str| m.servers.iter().any(|s| s.program == BLKD && s.receives.iter().any(|r| r == e));
    for (i, s) in m.servers.iter().enumerate() {
        if let Some(k) = s.handed.iter().position(|h| ranges(&h.endpoint)) {
            return Err(at(format!("servers[{i}].handed[{k}].endpoint"), Why::BlkdHanded));
        }
    }
    for (i, s) in m.servers.iter().enumerate() {
        let own = if s.program == BLKD {
            RANGE_LABELS_ARG
        } else if s.volume.is_some() {
            LABELS_ARG
        } else {
            continue;
        };
        if let Some(k) = s.args.iter().position(|a| a.starts_with(own)) {
            return Err(at(format!("servers[{i}].args[{k}]"), Why::Argument));
        }
    }
    Ok(())
}

/// Every reference names something the manifest or the bundle holds.
fn references(m: &Manifest, machine: &Machine) -> Result<(), Refusal> {
    let label = |n: &String| m.labels.iter().any(|l| &l.name == n);
    let labels = |list: &[String], path: &dyn Fn(usize) -> String| -> Result<(), Refusal> {
        if list.len() > MAX_LABELS {
            return Err(at(path(0), Why::TooManyLabels));
        }
        match list.iter().position(|n| !label(n)) {
            Some(k) => Err(at(path(k), Why::Unknown)),
            None => match (1..list.len()).find(|k| list[..*k].contains(&list[*k])) {
                Some(k) => Err(at(path(k), Why::Twice)),
                None => Ok(()),
            },
        }
    };
    for (i, l) in m.labels.iter().enumerate() {
        if !m.principals.iter().any(|p| p.name == l.owner) {
            return Err(at(format!("labels[{i}].owner"), Why::Unknown));
        }
    }
    for (i, v) in m.volumes.iter().enumerate() {
        labels(&v.labels, &|k| format!("volumes[{i}].labels[{k}]"))?;
        if !(0..=i64::from(u8::MAX)).contains(&v.partition) {
            return Err(at(format!("volumes[{i}].partition"), Why::Value));
        }
    }
    let volume = |n: &str| m.volumes.iter().any(|v| v.name == n);
    let received = |e: &str| m.servers.iter().any(|s| s.receives.iter().any(|r| r == e));
    for (i, s) in m.servers.iter().enumerate() {
        if !machine.entries.iter().any(|(name, _)| *name == s.program) || s.program == MANIFEST {
            return Err(at(format!("servers[{i}].program"), Why::Unknown));
        }
        labels(&s.labels, &|k| format!("servers[{i}].labels[{k}]"))?;
        if s.volume.as_ref().is_some_and(|v| !volume(v)) {
            return Err(at(format!("servers[{i}].volume"), Why::Unknown));
        }
        for (k, h) in s.handed.iter().enumerate() {
            if !received(&h.endpoint) {
                return Err(at(format!("servers[{i}].handed[{k}].endpoint"), Why::Unknown));
            }
            // A root badge, below every badge a server mints (servers/serving.md R27), and one
            // holder per badge at an endpoint, so each system caller has a share of its own.
            let mut earlier = m.servers[..=i].iter().enumerate().flat_map(|(j, t)| {
                let upto = if j == i { k } else { t.handed.len() };
                &t.handed[..upto]
            });
            let twice = earlier.any(|g| g.endpoint == h.endpoint && g.badge == h.badge);
            if h.badge == 0 || h.badge >= FIRST_MINTED_BADGE || twice {
                return Err(at(format!("servers[{i}].handed[{k}].badge"), Why::Badge));
            }
        }
        for (k, a) in s.args.iter().enumerate() {
            if a.contains('\0') {
                return Err(at(format!("servers[{i}].args[{k}]"), Why::Argument));
            }
        }
    }
    let mut accounts: Vec<u64> = Vec::new();
    for (i, p) in m.principals.iter().enumerate() {
        if p.account == 0 || accounts.contains(&p.account) {
            return Err(at(
                format!("principals[{i}].account"),
                if p.account == 0 { Why::Value } else { Why::Twice },
            ));
        }
        accounts.push(p.account);
        labels(&p.labels, &|k| format!("principals[{i}].labels[{k}]"))?;
        if let Some(k) =
            p.labels.iter().position(|n| !m.labels.iter().any(|l| &l.name == n && l.owner == p.name))
        {
            return Err(at(format!("principals[{i}].labels[{k}]"), Why::Unknown));
        }
        for (j, set) in p.label_sets.iter().enumerate() {
            labels(&set.labels, &|k| format!("principals[{i}].label_sets[{j}].labels[{k}]"))?;
        }
        if let Some(home) = &p.home {
            let fine = home
                .split_once(':')
                .is_some_and(|(v, path)| volume(v) && redoubt_rt::path::is_clean_absolute(path));
            if !fine {
                return Err(at(format!("principals[{i}].home"), Why::Value));
            }
        }
        for (j, n) in p.net.iter().enumerate() {
            if !prefix(&n.prefix) {
                return Err(at(format!("principals[{i}].net[{j}].prefix"), Why::Value));
            }
            if let Some(k) = n.ports.iter().position(|p| !(1..=i64::from(u16::MAX)).contains(p)) {
                return Err(at(format!("principals[{i}].net[{j}].ports[{k}]"), Why::Value));
            }
        }
    }
    Ok(())
}

/// An IP prefix, `ADDRESS/LENGTH`, the length within the address's bits.
fn prefix(text: &str) -> bool {
    let Some((addr, len)) = text.split_once('/') else { return false };
    let bits = match addr.parse::<IpAddr>() {
        Ok(IpAddr::V4(_)) => 32,
        Ok(IpAddr::V6(_)) => 128,
        Err(_) => return false,
    };
    let canonical = len == "0" || (!len.starts_with('0') && !len.is_empty());
    canonical && len.parse::<u8>().is_ok_and(|n| n <= bits)
}

/// Each `devices` entry matched to the handles the kernel says name that device, and each server's
/// devices placed under its names (kernel/devices.md, "Which process gets which device").
fn devices(m: &Manifest, machine: &Machine) -> Result<Vec<Vec<(String, Handle)>>, Refusal> {
    // Each entry's handles: its register region's and its interrupt's.
    let mut found: Vec<(Option<Handle>, Option<Handle>)> = Vec::new();
    for (i, d) in m.devices.iter().enumerate() {
        let mmio = match d.base {
            None => None,
            Some(base) => {
                let hit = machine
                    .devices
                    .iter()
                    .find(|(_, info)| matches!(info, DeviceInfo::Mmio { base: b, .. } if *b == base));
                let Some((handle, DeviceInfo::Mmio { dma, .. })) = hit else {
                    return Err(at(format!("devices[{i}].base"), Why::Unmatched));
                };
                if *dma != d.dma {
                    return Err(at(format!("devices[{i}].dma"), Why::DmaMismatch));
                }
                Some(*handle)
            }
        };
        if d.base.is_none() && d.dma {
            return Err(at(format!("devices[{i}].dma"), Why::DmaMismatch));
        }
        let irq = match d.irq {
            None => None,
            Some(irq) => {
                let hit = machine
                    .devices
                    .iter()
                    .find(|(_, info)| matches!(info, DeviceInfo::Irq(n) if i64::from(*n) == irq));
                Some(hit.ok_or_else(|| at(format!("devices[{i}].irq"), Why::Unmatched))?.0)
            }
        };
        let earlier = |j: usize| m.devices[j].base.is_some() && m.devices[j].base == d.base;
        if (0..i).any(earlier) {
            return Err(at(format!("devices[{i}].base"), Why::SplitDevice));
        }
        if (0..i).any(|j| m.devices[j].irq.is_some() && m.devices[j].irq == d.irq) {
            return Err(at(format!("devices[{i}].irq"), Why::SplitDevice));
        }
        found.push((mmio, irq));
    }
    let mut holder: Vec<Option<usize>> = alloc::vec![None; m.devices.len()];
    let mut placements = Vec::new();
    for (i, s) in m.servers.iter().enumerate() {
        let mut placed = Vec::new();
        for (k, u) in s.devices.iter().enumerate() {
            let path = || format!("servers[{i}].devices[{k}].device");
            let d =
                m.devices.iter().position(|d| d.name == u.device).ok_or_else(|| at(path(), Why::Unknown))?;
            if holder[d].replace(i).is_some() {
                return Err(at(path(), Why::HeldTwice));
            }
            let (mmio, irq) = found[d];
            if let Some(h) = mmio {
                placed.push((u.name.clone(), h));
            }
            if let Some(h) = irq {
                placed.push((format!("{}{IRQ_SUFFIX}", u.name), h));
            }
        }
        placements.push(placed);
    }
    Ok(placements)
}

/// Limits a process can run in: at least one process and some weight, within what the kernel
/// takes.
fn budgets(m: &Manifest) -> Result<(), Refusal> {
    let fine = |b: &Budget| {
        (1..=i64::from(u32::MAX)).contains(&b.processes) && (1..=i64::from(u32::MAX)).contains(&b.weight)
    };
    if let Some(i) = m.servers.iter().position(|s| !fine(&s.budget)) {
        return Err(at(format!("servers[{i}].budget"), Why::Budget));
    }
    for (i, p) in m.principals.iter().enumerate() {
        if !fine(&p.budget) {
            return Err(at(format!("principals[{i}].budget"), Why::Budget));
        }
        if let Some(j) = p.label_sets.iter().position(|s| !fine(&s.budget)) {
            return Err(at(format!("principals[{i}].label_sets[{j}].budget"), Why::Budget));
        }
    }
    Ok(())
}

/// The servers' budgets fit in what `system` has free: pages (each budget's own page included,
/// which `system` pays), processes and weight (servers/init.md, Weights).
fn fit(m: &Manifest, system: &Usage) -> Result<(), Refusal> {
    let total = |f: &dyn Fn(&Server) -> u64| m.servers.iter().fold(0u64, |sum, s| sum.saturating_add(f(s)));
    let pages = total(&|s| s.budget.pages.saturating_add(1));
    let processes = total(&|s| s.budget.processes as u64);
    let weight = total(&|s| s.budget.weight as u64);
    let free = [
        ("pages", pages, system.pages_limit.saturating_sub(system.pages_usage)),
        ("processes", processes, u64::from(system.processes_limit.saturating_sub(system.processes_usage))),
        ("weight", weight, u64::from(system.weight_limit.saturating_sub(system.weight_carved))),
    ];
    match free.iter().find(|(_, need, free)| need > free) {
        Some((what, need, free)) => Err(Refusal::SystemFit { what, need: *need, free: *free }),
        None => Ok(()),
    }
}

/// The arguments `init` gives server `s`: its entry's own, then, for `bootfsd`, the `public` list,
/// which `bootfsd` builds its table from (servers/bootfsd.md, "Started by `init`"); for a
/// volume's server, `labels=` its volume's label ids, absent when the set is empty; and for
/// `blkd`, `labels.P=` each labelled volume's ids, P its GPT entry (servers/blkd.md, "Ranges and
/// badges"). A label the manifest does not define is left out: the check refused it before.
pub fn args(m: &Manifest, s: &Server) -> Vec<String> {
    let ids = |names: &[String]| {
        let ids: Vec<String> = names
            .iter()
            .filter_map(|n| m.labels.iter().find(|l| &l.name == n))
            .map(|l| format!("{}", l.id))
            .collect();
        ids.join(",")
    };
    let mut args = s.args.clone();
    if s.program == BOOTFSD {
        args.extend(m.public.iter().cloned());
    }
    if let Some(v) = s.volume.as_ref().and_then(|n| m.volumes.iter().find(|v| &v.name == n)) {
        if !v.labels.is_empty() {
            args.push(format!("{LABELS_ARG}{}", ids(&v.labels)));
        }
    }
    if s.program == BLKD {
        for v in m.volumes.iter().filter(|v| !v.labels.is_empty()) {
            args.push(format!("{RANGE_LABELS_ARG}{}={}", v.partition, ids(&v.labels)));
        }
    }
    args
}

/// `public` names bundle entries, each once, never the manifest, and a `bootfsd` serves them
/// (servers/bootfsd.md, "Started by `init`"). A `bootfsd` entry's own arguments are only its
/// `buckets=N`: the names it serves come from `public` alone.
fn public(m: &Manifest, machine: &Machine) -> Result<(), Refusal> {
    for (i, name) in m.public.iter().enumerate() {
        let path = || format!("public[{i}]");
        if name == MANIFEST {
            return Err(at(path(), Why::PublicManifest));
        }
        if !redoubt_rt::path::valid_name(name) || !machine.entries.iter().any(|(e, _)| e == name) {
            return Err(at(path(), Why::Unknown));
        }
        if m.public[..i].contains(name) {
            return Err(at(path(), Why::Twice));
        }
    }
    if !m.public.is_empty() && !m.servers.iter().any(|s| s.program == BOOTFSD) {
        return Err(at(String::from("public"), Why::NoBootfsd));
    }
    for (i, s) in m.servers.iter().enumerate().filter(|(_, s)| s.program == BOOTFSD) {
        if let Some(k) = s.args.iter().position(|a| !a.starts_with(BUCKETS_ARG)) {
            return Err(at(format!("servers[{i}].args[{k}]"), Why::Argument));
        }
    }
    Ok(())
}

/// Each server's startup block holds its handles and arguments within one page and
/// `MAX_START_HANDLES`: the block written here is the one the boot writes, with the same names,
/// a console connection at `/dev/cons`, and the image.
fn blocks(m: &Manifest, machine: &Machine) -> Result<(), Refusal> {
    for (i, s) in m.servers.iter().enumerate() {
        let refused = || at(format!("servers[{i}]"), Why::Block);
        let mut names: Vec<String> = s.receives.clone();
        names.extend(s.handed.iter().map(|h| h.endpoint.clone()));
        if s.volume.is_some() {
            names.push(String::from(VOLUME));
        }
        for d in &s.devices {
            names.push(d.name.clone());
            names.push(format!("{}{IRQ_SUFFIX}", d.name));
        }
        let count = names.len() + 1;
        if count > MAX_START_HANDLES {
            return Err(refused());
        }
        let mut block = StartupBuilder::new(count as u32);
        let handle = |n: usize| Handle::new(n as u32).ok_or_else(refused);
        for (n, name) in names.iter().enumerate() {
            block.handle(name, handle(n + 1)?);
        }
        block.namespace("/dev/cons", handle(count)?);
        for a in args(m, s) {
            block.arg(&a);
        }
        let len = machine.entries.iter().find(|(e, _)| *e == s.program).map_or(0, |(_, len)| *len);
        block.image(stub::IMAGE_AT, len);
        block.finish().map_err(|_| refused())?;
    }
    Ok(())
}

/// Every principal's login and approval key, decoded (R35 (key separation)).
fn keys(m: &Manifest) -> Result<Vec<(String, [u8; KEY_LEN])>, Refusal> {
    let mut keys = Vec::new();
    for (i, p) in m.principals.iter().enumerate() {
        let lists = [("ssh_keys", &p.ssh_keys), ("approval_keys", &p.approval_keys)];
        for (member, list) in lists {
            for (k, text) in list.iter().enumerate() {
                let path = format!("principals[{i}].{member}[{k}]");
                let key = sshkey::ed25519(text).ok_or_else(|| at(path.clone(), Why::Key))?;
                keys.push((path, key));
            }
        }
    }
    Ok(keys)
}

/// Every shared server (one whose arguments carry `buckets=N`) has N at least the domains
/// declared there: each principal's unlabelled set and every label set it works under, counted at
/// every shared server, plus one root badge per system caller (servers/init.md, Sizing).
fn buckets(m: &Manifest) -> Result<Vec<(usize, u32)>, Refusal> {
    let principals = m.principals.iter().fold(0u64, |sum, p| sum.saturating_add(domains(p) as u64));
    let mut out = Vec::new();
    for (i, s) in m.servers.iter().enumerate() {
        if !s.args.iter().any(|a| a.starts_with(BUCKETS_ARG)) {
            continue;
        }
        let args: Vec<&str> = s.args.iter().map(String::as_str).collect();
        let have = redoubt_rt::server::buckets(&args)
            .map_err(|_| at(format!("servers[{i}].args"), Why::BucketsArgument))?;
        let need = principals.saturating_add(callers(m, s)).min(u64::from(u32::MAX)) as u32;
        if have < need {
            return Err(Refusal::Buckets { at: format!("servers[{i}]"), have, need });
        }
        out.push((i, need));
    }
    Ok(out)
}

/// A principal's domains: its unlabelled set and each different label set it works under.
pub fn domains(p: &crate::manifest::Principal) -> usize {
    let mut sets: Vec<Vec<&str>> = alloc::vec![Vec::new()];
    for set in &p.label_sets {
        let mut labels: Vec<&str> = set.labels.iter().map(String::as_str).collect();
        labels.sort_unstable();
        if !sets.contains(&labels) {
            sets.push(labels);
        }
    }
    sets.len()
}

/// The system callers at `s`: one root badge per `handed` item naming an endpoint it receives
/// on, and `init`'s own if `init` calls it.
pub fn callers(m: &Manifest, s: &Server) -> u64 {
    let handed =
        m.servers.iter().flat_map(|t| &t.handed).filter(|h| s.receives.contains(&h.endpoint)).count();
    handed as u64 + u64::from(INIT_CALLS.contains(&s.program.as_str()))
}

/// `init`'s own badge at each server it calls ([`Plan::init_badges`]). Badges at an endpoint are
/// distinct (checked with the references), so one of the first `handed + 1` is free.
fn init_badges(m: &Manifest) -> Vec<(usize, u64)> {
    let mut out = Vec::new();
    for (i, s) in m.servers.iter().enumerate() {
        let Some(endpoint) = s.receives.first().filter(|_| INIT_CALLS.contains(&s.program.as_str())) else {
            continue;
        };
        let used: BTreeSet<u64> = m
            .servers
            .iter()
            .flat_map(|t| &t.handed)
            .filter(|h| &h.endpoint == endpoint)
            .map(|h| h.badge)
            .collect();
        let badge = (1..).find(|b| !used.contains(b)).expect("a finite set leaves a badge free");
        out.push((i, badge));
    }
    out
}

/// The bound's inputs, from the manifest and the machine.
fn counts(m: &Manifest, machine: &Machine) -> Counts {
    let image = |s: &Server| machine.entries.iter().find(|(e, _)| *e == s.program).map_or(0, |(_, len)| *len);
    Counts {
        servers: m.servers.len() as u64,
        endpoints: m.servers.iter().map(|s| s.receives.len() as u64).sum(),
        handed: m.servers.iter().map(|s| s.handed.len() as u64).sum(),
        stub_bytes: machine.stub_bytes as u64,
        largest_image_bytes: m.servers.iter().map(image).max().unwrap_or(0) as u64,
        stack_pages: machine.stack_pages as u64,
        handles_at_start: machine.handles_at_start as u64,
        arena_pages: machine.arena_pages as u64,
    }
}
