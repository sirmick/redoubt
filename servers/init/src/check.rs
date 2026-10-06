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
use redoubt_steward::hash::key_id;
use redoubt_steward::manifest::{
    Limits, Manifest as StewardManifest, PrincipalSpec, Sizes, lines as manifest_lines, quote, show_list,
};
use redoubt_sys::DeviceInfo;
use stub::MAX_STACK_PAGES;

use crate::bound::{self, Counts};
use crate::confine;
use crate::manifest::{Budget, Manifest, Server, Steward, Verity, Volume};
use crate::refusal::{Refusal, Why};
use crate::sshkey::{self, KEY_LEN};

/// The bundle entry that is the manifest. It is never public.
pub const MANIFEST: &str = "manifest";
/// The budgets `init` holds. No endpoint takes their names, and no `handed` item names one
/// (R33 (no server holds a system budget)): `init` hands `users` to the steward's entry alone, by
/// the manifest's `steward.server`, at step 6, never through `handed`.
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
/// The shared servers a session's namespace holds a connection to, one slot each in the steward's
/// binding table (servers/steward.md, "Two embedders and a reference"): `bootfsd`, the home
/// volume's `littlefsd`, the labelled volume's `littlefsd`, `ipd`, the console and the system
/// volume's `erofsd`.
/// The steward refuses to start on another count.
pub const STEWARD_SLOTS: u16 = 6;
/// The most rules a scope `ipd` grants holds (servers/ipd.md, "Scopes").
pub const IPD_RULES: usize = 8;
/// The startup-block name of the `users` budget, handed to the steward's entry alone.
pub const USERS: &str = "users";
/// The program that serves a disk's ranges: `init` mints each volume's range at its disk's.
pub const BLKD: &str = "blkd";
/// The startup-block name of a volume's range, handed to the server attaching it.
pub const VOLUME: &str = "volume";
/// The argument giving a volume's server its label ids (servers/littlefsd.md, "Volumes, connections
/// and labels").
pub const LABELS_ARG: &str = "labels=";
/// The prefix of the arguments giving `blkd` each labelled range's ids, `labels.P=ID,...` for
/// GPT entry P (servers/blkd.md, "Ranges and badges").
pub const RANGE_LABELS_ARG: &str = "labels.";
/// The argument naming the endpoint a `blkd` or a `verityd` receives on (servers/blkd.md, "Its
/// endpoint").
pub const ENDPOINT_ARG: &str = "endpoint=";
/// The program that verifies a volume (servers/verityd.md): `init` mints the volume's range at it
/// for the volume's server, and its own range at the disk's `blkd`.
pub const VERITYD: &str = "verityd";
/// The one badge a `verityd` serves, which `init` mints for its volume's server.
pub const VERIFIED_BADGE: u64 = 1;
/// The arguments giving a `verityd` the root and the data blocks it checks against, pinned, or
/// the key and the floor its volume's root block is checked against, signed.
pub const ROOT_ARG: &str = "root=";
pub const BLOCKS_ARG: &str = "blocks=";
pub const KEY_ARG: &str = "key=";
pub const FLOOR_ARG: &str = "floor=";
/// A signed volume's `key` naming the key the loader verified the bundle with, by name so no copy
/// of it drifts (servers/verityd.md, "The root block, and the two modes").
pub const BUNDLE_VOLUME_KEY: &str = "bundle";
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
    /// The stub's length in bytes.
    pub stub_bytes: usize,
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
    /// The key the loader verified the bundle with: what a signed volume's `"key": "bundle"`
    /// hands its verifier ([`args`]).
    pub bundle_key: [u8; KEY_LEN],
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
    verifiers(m)?;
    let placements = devices(m, machine)?;
    budgets(m)?;
    fit(m, &machine.system)?;
    public(m, machine)?;
    blocks(m, machine, &bundle_key)?;
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
    Ok(Plan { placements, keys, buckets, init_badges: init_badges(m), bound, bundle_key })
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

/// The `blkd` serving volume `v`'s disk, whose endpoint its range is minted at: the one its
/// `disk` names, or, when it names none, the manifest's only `blkd`.
pub fn blkd<'m>(m: &'m Manifest, v: &Volume) -> Option<&'m Server> {
    let mut found = m.servers.iter().filter(|s| s.program == BLKD);
    match &v.disk {
        Some(disk) => found.find(|s| &s.name == disk),
        None => found.next().filter(|_| found.next().is_none()),
    }
}

/// Whether volume `v` is on the disk the `blkd` `s` serves.
pub fn on_disk(m: &Manifest, v: &Volume, s: &Server) -> bool { blkd(m, v).is_some_and(|b| b.name == s.name) }

/// The `verityd` entry verifying volume `v`, if `v` is verified.
pub fn verifier<'m>(m: &'m Manifest, v: &Volume) -> Option<&'m Server> {
    let name = &v.verity.as_ref()?.server;
    m.servers.iter().find(|s| &s.name == name)
}

/// The volume server `s` verifies, if any names it.
pub fn verified<'m>(m: &'m Manifest, s: &Server) -> Option<&'m Volume> {
    m.volumes.iter().find(|v| v.verity.as_ref().is_some_and(|verity| verity.server == s.name))
}

/// Where `init` mints server `s`'s `volume` handle, and its badge there: for a volume's server,
/// the badge of its GPT entry + 1 at its disk's `blkd` (servers/blkd.md, "Ranges and badges"), or,
/// for a verified volume, [`VERIFIED_BADGE`] at its verifier's endpoint; for a verifier, its
/// volume's range at the disk's `blkd` (servers/verityd.md).
pub fn range<'m>(m: &'m Manifest, s: &Server) -> Option<(&'m str, u64)> {
    let at_blkd = |v: &Volume| {
        let endpoint = blkd(m, v)?.receives.first()?;
        Some((endpoint.as_str(), v.partition as u64 + 1))
    };
    match s.volume.as_ref().and_then(|n| m.volumes.iter().find(|v| &v.name == n)) {
        Some(v) => match verifier(m, v) {
            Some(verifier) => verifier.receives.first().map(|e| (e.as_str(), VERIFIED_BADGE)),
            None => at_blkd(v),
        },
        None => verified(m, s).and_then(at_blkd),
    }
}

/// Whether server `t` holds volume `v`'s range at its disk's `blkd`: the verifier of a verified
/// volume, the server attaching any other.
pub fn holds_range(m: &Manifest, v: &Volume, t: &Server) -> bool {
    match verifier(m, v) {
        Some(verifier) => verifier.name == t.name,
        None => t.volume.as_ref() == Some(&v.name),
    }
}

/// Each volume is one GPT entry of its disk, served by at most one server (R47 (one volume per
/// instance)), and a volume a server attaches has its disk's `blkd`, receiving on an endpoint, to
/// mint its range at. With more than one `blkd`, every volume names its disk. No entry is handed a
/// badge where any `blkd` receives, since `blkd` resolves each badge there to a volume's range,
/// and no entry carries an argument `init` passes itself: `labels=` on a volume's server,
/// `labels.` on `blkd`.
fn volumes(m: &Manifest) -> Result<(), Refusal> {
    let disks = m.servers.iter().filter(|s| s.program == BLKD).count();
    for (i, v) in m.volumes.iter().enumerate() {
        if disks > 1 && v.disk.is_none() {
            return Err(at(format!("volumes[{i}].disk"), Why::NoDisk));
        }
        let disk = |w: &Volume| blkd(m, w).map(|b| &b.name);
        if m.volumes[..i].iter().any(|w| w.partition == v.partition && disk(w) == disk(v)) {
            return Err(at(format!("volumes[{i}].partition"), Why::Twice));
        }
    }
    for (i, s) in m.servers.iter().enumerate() {
        let Some(volume) = &s.volume else { continue };
        if m.servers[..i].iter().any(|t| t.volume.as_ref() == Some(volume)) {
            return Err(at(format!("servers[{i}].volume"), Why::Twice));
        }
        let served = |v: &Volume| blkd(m, v).is_some_and(|b| !b.receives.is_empty());
        if !m.volumes.iter().any(|v| &v.name == volume && served(v)) {
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
    // A blkd receives where its `endpoint=` says, and `init` mints its ranges at its first
    // endpoint: the two are one, or a range would be minted where nobody serves it.
    for (i, s) in m.servers.iter().enumerate().filter(|(_, s)| s.program == BLKD) {
        let mut named = s.args.iter().enumerate().filter(|(_, a)| a.starts_with(ENDPOINT_ARG));
        let first = s.receives.first().map(|r| format!("{ENDPOINT_ARG}{r}"));
        match (named.next(), named.next()) {
            (Some((_, a)), None) if Some(a) == first.as_ref() => {}
            (Some(_), Some((k, _))) | (Some((k, _)), None) => {
                return Err(at(format!("servers[{i}].args[{k}]"), Why::BlkdEndpoint));
            }
            (None, _) => return Err(at(format!("servers[{i}].args"), Why::BlkdEndpoint)),
        }
    }
    Ok(())
}

/// Each verified volume names a `verityd` entry that no other volume names, and each `verityd`
/// verifies one volume (servers/verityd.md, R76 (verified volumes)). A verifier receives on an
/// endpoint, attaches no volume itself, carries no argument (every one is `init`'s), has its
/// volume's label set, and has its volume's disk's `blkd`, receiving, to hold its range at. No
/// entry is handed a badge where a verifier receives: the one badge there is the volume's range,
/// which only `init` mints, for the volume's server.
fn verifiers(m: &Manifest) -> Result<(), Refusal> {
    for (i, v) in m.volumes.iter().enumerate() {
        let Some(verity) = &v.verity else { continue };
        let path = || format!("volumes[{i}].verity.server");
        if verifier(m, v).is_none_or(|s| s.program != VERITYD) {
            return Err(at(path(), Why::NotVerityd));
        }
        if m.volumes[..i].iter().any(|w| w.verity.as_ref().is_some_and(|w| w.server == verity.server)) {
            return Err(at(path(), Why::Twice));
        }
    }
    fn sorted(labels: &[String]) -> Vec<&str> {
        let mut set: Vec<&str> = labels.iter().map(String::as_str).collect();
        set.sort_unstable();
        set
    }
    for (i, s) in m.servers.iter().enumerate().filter(|(_, s)| s.program == VERITYD) {
        let Some(v) = verified(m, s) else { return Err(at(format!("servers[{i}]"), Why::NoVolume)) };
        if s.volume.is_some() {
            return Err(at(format!("servers[{i}].volume"), Why::Verifier));
        }
        if s.receives.is_empty() {
            return Err(at(format!("servers[{i}].receives"), Why::Verifier));
        }
        if !s.args.is_empty() {
            return Err(at(format!("servers[{i}].args[0]"), Why::Argument));
        }
        if sorted(&s.labels) != sorted(&v.labels) {
            return Err(at(format!("servers[{i}].labels"), Why::VerifierLabels));
        }
        if blkd(m, v).is_none_or(|b| b.receives.is_empty()) {
            return Err(at(format!("servers[{i}]"), Why::NoBlkd));
        }
    }
    let verifies =
        |e: &str| m.servers.iter().any(|s| s.program == VERITYD && s.receives.iter().any(|r| r == e));
    for (i, s) in m.servers.iter().enumerate() {
        if let Some(k) = s.handed.iter().position(|h| verifies(&h.endpoint)) {
            return Err(at(format!("servers[{i}].handed[{k}].endpoint"), Why::VerifierHanded));
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
        if v.disk.is_some() && blkd(m, v).is_none() {
            return Err(at(format!("volumes[{i}].disk"), Why::Unknown));
        }
        if let Some(verity) = &v.verity {
            if verifier(m, v).is_none() {
                return Err(at(format!("volumes[{i}].verity.server"), Why::Unknown));
            }
            let field = |name: &str| format!("volumes[{i}].verity.{name}");
            match verity {
                Verity { root: Some(root), blocks: Some(blocks), key: None, floor: None, .. } => {
                    if redoubt_verity::from_hex(root).is_none() {
                        return Err(at(field("root"), Why::Value));
                    }
                    // The count `verityd` takes: at least one block, its tree beside it in `u64`
                    // sectors.
                    if redoubt_verity::Geometry::new(*blocks).is_none() {
                        return Err(at(field("blocks"), Why::Value));
                    }
                }
                Verity { root: None, blocks: None, key: Some(key), floor: Some(_), .. } => {
                    if key != BUNDLE_VOLUME_KEY && redoubt_verity::from_hex(key).is_none() {
                        return Err(at(field("key"), Why::Value));
                    }
                }
                _ => return Err(at(format!("volumes[{i}].verity"), Why::VerityMode)),
            }
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
    if let Some(st) = &m.steward {
        let Some(i) = m.servers.iter().position(|s| s.name == st.server) else {
            return Err(at("steward.server".into(), Why::Unknown));
        };
        // The manifest lines are `init`'s to write: the entry's own arguments carry none.
        if let Some(k) = m.servers[i].args.iter().position(|a| !a.starts_with(BUCKETS_ARG)) {
            return Err(at(format!("servers[{i}].args[{k}]"), Why::Argument));
        }
        // Every home's and vault's server is one the steward is handed, so each line names a
        // handle it holds.
        let handed = |e: &str| m.servers[i].handed.iter().any(|h| h.endpoint == e);
        for (j, p) in m.principals.iter().enumerate() {
            // The scope the steward asks `ipd` for: IPv4 connect rules, one per port or one for
            // every port, at most `ipd`'s eight.
            let rules = p.net.iter().map(|n| n.ports.len().max(1)).sum::<usize>();
            let v4 = p.net.iter().all(|n| {
                n.prefix.split_once('/').is_some_and(|(a, _)| a.parse::<core::net::Ipv4Addr>().is_ok())
            });
            if rules > IPD_RULES || !v4 {
                return Err(at(format!("principals[{j}].net"), Why::Value));
            }
            let home = p.home.as_deref().and_then(|h| h.split_once(':'));
            if home.is_some_and(|(v, _)| volume_handle(m, v).is_none_or(|e| !handed(e))) {
                return Err(at(format!("principals[{j}].home"), Why::Unknown));
            }
            for (k, set) in p.label_sets.iter().enumerate() {
                if vault_handle(m, &set.labels).is_some_and(|e| !handed(e)) {
                    return Err(at(format!("principals[{j}].label_sets[{k}]"), Why::Unknown));
                }
            }
        }
    }
    if let Some(c) = &m.console {
        // The console session is the steward's to open, for a principal the manifest names.
        if m.steward.is_none() || !m.principals.iter().any(|p| &p.name == c) {
            return Err(at("console".into(), Why::Unknown));
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
/// takes; a first-thread stack, and a heap cap if any, that leave room in the server's budget; and
/// the steward's sizes, each with pages too, within every principal's smallest share.
fn budgets(m: &Manifest) -> Result<(), Refusal> {
    let fine = |b: &Budget| {
        (1..=i64::from(u32::MAX)).contains(&b.processes) && (1..=i64::from(u32::MAX)).contains(&b.weight)
    };
    if let Some(i) = m.servers.iter().position(|s| !fine(&s.budget)) {
        return Err(at(format!("servers[{i}].budget"), Why::Budget));
    }
    if let Some(i) = m.servers.iter().position(|s| {
        s.stack_pages == 0 || s.stack_pages > MAX_STACK_PAGES as u64 || s.stack_pages >= s.budget.pages
    }) {
        return Err(at(format!("servers[{i}].stack_pages"), Why::Stack));
    }
    if let Some(i) = m.servers.iter().position(|s| {
        s.heap_pages.is_some_and(|heap| heap == 0 || u64::from(heap) + s.stack_pages >= s.budget.pages)
    }) {
        return Err(at(format!("servers[{i}].heap_pages"), Why::Heap));
    }
    for (i, p) in m.principals.iter().enumerate() {
        if !fine(&p.budget) {
            return Err(at(format!("principals[{i}].budget"), Why::Budget));
        }
    }
    let Some(st) = &m.steward else { return Ok(()) };
    // Every size a process can run in, and a budget object's cost at least its page.
    let z = &st.sizes;
    let sizes = [
        ("session", &z.session),
        ("agent", &z.agent),
        ("sub_agent", &z.sub_agent),
        ("crossing", &z.crossing),
    ];
    for (name, b) in sizes {
        if !fine(b) || b.pages == 0 {
            return Err(at(format!("steward.sizes.{name}"), Why::Budget));
        }
    }
    if z.cost == 0 {
        return Err(at("steward.sizes.cost".into(), Why::Budget));
    }
    // Each size fits the smallest sub-budget the steward carves: an equal share of a principal's
    // budget per domain, less a budget's own cost (servers/steward.md, "Fixed sub-budgets per
    // label set").
    for (i, p) in m.principals.iter().enumerate() {
        let n = domains(p) as u64;
        let share = (p.budget.pages / n).saturating_sub(z.cost);
        let (processes, weight) = (p.budget.processes as u64 / n, p.budget.weight as u64 / n);
        let over = |b: &Budget| b.pages > share || b.processes as u64 > processes || b.weight as u64 > weight;
        if let Some((name, _)) = sizes.iter().find(|(_, b)| over(b)) {
            return Err(at(format!("principals[{i}].budget"), Why::Sizes(name)));
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
/// volume's server, `labels=` its volume's label ids, absent when the set is empty; and for a
/// `blkd`, `labels.P=` the ids of each labelled volume on its disk, P its GPT entry
/// (servers/blkd.md, "Ranges and badges"); for a volume's `verityd`, `endpoint=` its first
/// endpoint, `labels=` its volume's ids as its server's, and `root=` and `blocks=`, or `key=`
/// (`bundle_key` in hex for `bundle`) and `floor=`, from the volume's `verity`
/// (servers/verityd.md, "Arguments"); for the steward's entry, the manifest lines
/// ([`steward_lines`]). A label the manifest does not define is left out: the check refused it
/// before.
pub fn args(m: &Manifest, s: &Server, bundle_key: &[u8; KEY_LEN]) -> Vec<String> {
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
    if let Some((v, verity)) = verified(m, s).and_then(|v| Some((v, v.verity.as_ref()?))) {
        if let Some(e) = s.receives.first() {
            args.push(format!("{ENDPOINT_ARG}{e}"));
        }
        if !v.labels.is_empty() {
            args.push(format!("{LABELS_ARG}{}", ids(&v.labels)));
        }
        // The check let through one mode, both of its members.
        if let (Some(root), Some(blocks)) = (&verity.root, verity.blocks) {
            args.push(format!("{ROOT_ARG}{root}"));
            args.push(format!("{BLOCKS_ARG}{blocks}"));
        }
        if let (Some(key), Some(floor)) = (&verity.key, verity.floor) {
            match key.as_str() {
                BUNDLE_VOLUME_KEY => {
                    let hex: String = bundle_key.iter().map(|b| format!("{b:02x}")).collect();
                    args.push(format!("{KEY_ARG}{hex}"));
                }
                key => args.push(format!("{KEY_ARG}{key}")),
            }
            args.push(format!("{FLOOR_ARG}{floor}"));
        }
    }
    if s.program == BLKD {
        for v in m.volumes.iter().filter(|v| !v.labels.is_empty() && on_disk(m, v, s)) {
            args.push(format!("{RANGE_LABELS_ARG}{}={}", v.partition, ids(&v.labels)));
        }
    }
    if let Some(st) = m.steward.as_ref().filter(|_| is_steward(m, s)) {
        args.extend(steward_lines(m, st));
    }
    args
}

/// Whether `s` is the entry the manifest's `steward.server` names: the one `init` hands `users`.
pub fn is_steward(m: &Manifest, s: &Server) -> bool {
    m.steward.as_ref().is_some_and(|st| st.server == s.name)
}

/// The manifest lines the steward is started with, one per argument (servers/steward.md, "The
/// manifest lines"), written by the policy core's own writer: each principal with its keys' ids,
/// its owned labels' ids, and its domains, its unlabelled set first and then each different label
/// set, as [`domains`] counts them; `keyd []`, since the live key-separation checks are `init`'s
/// and `sshd`'s (R35); the steward's [`STEWARD_SLOTS`]; and the sizes. A key or label the check
/// refused is left out: the check ran first.
pub fn steward_lines(m: &Manifest, st: &Steward) -> Vec<String> {
    let ids = |names: &[String]| -> Vec<u64> {
        let mut ids: Vec<u64> =
            names.iter().filter_map(|n| m.labels.iter().find(|l| &l.name == n)).map(|l| l.id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let keys = |texts: &[String]| -> Vec<u64> {
        texts.iter().filter_map(|t| sshkey::ed25519(t)).map(|k| key_id(&k)).collect()
    };
    let limits =
        |b: &Budget| Limits { pages: b.pages, processes: b.processes as u64, weight: b.weight as u64 };
    let principals = m
        .principals
        .iter()
        .map(|p| {
            let mut sets: Vec<Vec<u64>> = alloc::vec![Vec::new()];
            for set in &p.label_sets {
                let set = ids(&set.labels);
                if !sets.contains(&set) {
                    sets.push(set);
                }
            }
            PrincipalSpec {
                name: p.name.clone(),
                account: p.account,
                login_keys: keys(&p.ssh_keys),
                approval_keys: keys(&p.approval_keys),
                owned: ids(&p.labels),
                label_sets: sets,
                top: limits(&p.budget),
            }
        })
        .collect();
    let z = &st.sizes;
    let lines = StewardManifest {
        principals,
        keyd_keys: Vec::new(),
        servers: STEWARD_SLOTS,
        sizes: Sizes {
            session: limits(&z.session),
            agent: limits(&z.agent),
            sub_agent: limits(&z.sub_agent),
            crossing: limits(&z.crossing),
            budget_cost: z.cost,
        },
    };
    let mut out = manifest_lines(&lines);
    out.extend(steward_own_lines(m));
    out
}

/// The server attaching volume `name`, by the named handle its first endpoint is handed as.
fn volume_handle<'m>(m: &'m Manifest, name: &str) -> Option<&'m str> {
    let s = m.servers.iter().find(|s| s.volume.as_deref() == Some(name))?;
    s.receives.first().map(String::as_str)
}

/// The labelled volume whose label set is `set` (names, any order), by its server's handle.
fn vault_handle<'m>(m: &'m Manifest, set: &[String]) -> Option<&'m str> {
    let mut want: Vec<&str> = set.iter().map(String::as_str).collect();
    want.sort_unstable();
    let v = m.volumes.iter().find(|v| {
        let mut have: Vec<&str> = v.labels.iter().map(String::as_str).collect();
        have.sort_unstable();
        !have.is_empty() && have == want
    })?;
    volume_handle(m, &v.name)
}

/// The steward's own lines, after the core's (servers/steward.md, "The manifest lines"): each
/// label's name and id; each principal's home, at the handle of its volume's server; the labelled
/// volume of each label set it works under that has one; its network scope, in the manifest's
/// prefix-and-ports form (`*` for every port); and the console's principal, if the manifest names
/// one.
pub fn steward_own_lines(m: &Manifest) -> Vec<String> {
    let q = |s: &str| quote(s.as_bytes());
    let mut out: Vec<String> = m.labels.iter().map(|l| format!("label {} id={}", q(&l.name), l.id)).collect();
    for p in &m.principals {
        let home = p.home.as_deref().and_then(|h| h.split_once(':'));
        if let Some((handle, path)) = home.and_then(|(v, path)| Some((volume_handle(m, v)?, path))) {
            out.push(format!("home {} handle={handle} path={path}", q(&p.name)));
        }
        let mut seen: Vec<Vec<u64>> = Vec::new();
        for set in &p.label_sets {
            let mut ids: Vec<u64> = set
                .labels
                .iter()
                .filter_map(|n| m.labels.iter().find(|l| &l.name == n))
                .map(|l| l.id)
                .collect();
            ids.sort_unstable();
            ids.dedup();
            if ids.is_empty() || seen.contains(&ids) {
                continue;
            }
            if let Some(handle) = vault_handle(m, &set.labels) {
                out.push(format!("vault {} labels={} handle={handle}", q(&p.name), show_list(&ids)));
            }
            seen.push(ids);
        }
        if !p.net.is_empty() {
            let rules: Vec<String> = p
                .net
                .iter()
                .map(|n| {
                    let ports: Vec<String> = n.ports.iter().map(|p| format!("{p}")).collect();
                    let ports = if ports.is_empty() { String::from("*") } else { ports.join(",") };
                    format!("{}:{ports}", n.prefix)
                })
                .collect();
            out.push(format!("net {} {}", q(&p.name), rules.join(" ")));
        }
    }
    if let Some(c) = &m.console {
        out.push(format!("console {}", q(c)));
    }
    out
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
/// a console connection at `/dev/cons`, the image, the heap cap and the tag.
fn blocks(m: &Manifest, machine: &Machine, bundle_key: &[u8; KEY_LEN]) -> Result<(), Refusal> {
    for (i, s) in m.servers.iter().enumerate() {
        let refused = || at(format!("servers[{i}]"), Why::Block);
        let mut names: Vec<String> = s.receives.clone();
        names.extend(s.handed.iter().map(|h| h.endpoint.clone()));
        if s.volume.is_some() || verified(m, s).is_some() {
            names.push(String::from(VOLUME));
        }
        if is_steward(m, s) {
            names.push(String::from(USERS));
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
        for a in args(m, s, bundle_key) {
            block.arg(&a);
        }
        let len = machine.entries.iter().find(|(e, _)| *e == s.program).map_or(0, |(_, len)| *len);
        // The tag is the server's place, as at launch.
        block.image(stub::IMAGE_AT, len).heap_pages(s.heap_pages.unwrap_or(0)).tag((i + 1) as u16);
        block.finish().map_err(|_| refused())?;
    }
    Ok(())
}

/// Every principal's login and approval key, decoded (R35 (key separation)), each once across
/// every principal's lists: a key that logs one principal in and approves for another, or is
/// listed twice, is refused here, before any server runs (the steward's core refuses it too).
fn keys(m: &Manifest) -> Result<Vec<(String, [u8; KEY_LEN])>, Refusal> {
    let mut keys: Vec<(String, [u8; KEY_LEN])> = Vec::new();
    for (i, p) in m.principals.iter().enumerate() {
        let lists = [("ssh_keys", &p.ssh_keys), ("approval_keys", &p.approval_keys)];
        for (member, list) in lists {
            for (k, text) in list.iter().enumerate() {
                let path = format!("principals[{i}].{member}[{k}]");
                let key = sshkey::ed25519(text).ok_or_else(|| at(path.clone(), Why::Key))?;
                if keys.iter().any(|(_, seen)| *seen == key) {
                    return Err(at(path, Why::Twice));
                }
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
        // Each `handed` item, and each volume's range badge (a verifier's included), is a badged
        // handle `init` mints.
        handed: m
            .servers
            .iter()
            .map(|s| s.handed.len() as u64 + u64::from(s.volume.is_some() || verified(m, s).is_some()))
            .sum(),
        stub_bytes: machine.stub_bytes as u64,
        largest_image_bytes: m.servers.iter().map(image).max().unwrap_or(0) as u64,
        stack_pages: m.servers.iter().map(|s| s.stack_pages).max().unwrap_or(0),
        handles_at_start: machine.handles_at_start as u64,
        arena_pages: machine.arena_pages as u64,
    }
}
