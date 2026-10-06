//! The confinement check (servers/init.md, "The confinement check"; R34 (confined placement)):
//! with `confined` set, two domains with differing label sets share nothing. Each sharing kind
//! is its own check with its own reason, in the page's order: endpoint, volume, network, device,
//! server instance.
//!
//! The domains are each `servers` entry, under its `labels`, and each label set a principal
//! works under, its unlabelled one included. The manifest does not route principals to servers
//! (the steward does, within a label set by this same rule), so a shared server, one sized with
//! `buckets=N`, is used by every principal domain with its own label set: only such a domain may
//! later be granted a connection there. A server is also used by every server handed one of its
//! endpoints; `blkd` by every server holding a volume's range at it, the server attaching the
//! volume or, for a verified volume, its `verityd`; and a `verityd` by the server attaching its
//! volume, whose range `init` mints at the verifier's endpoint. So a confined disk holds one label
//! set's volumes, and each label set reading a verified volume has its own verifier. The steward
//! and `sshd` serve every domain by design and are exempt, by program name.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::check::{BLKD, BUCKETS_ARG, holds_range, on_disk, verified};
use crate::manifest::{Manifest, Server};
use crate::refusal::{Refusal, Sharing};

/// The programs that are network instances.
const NETWORK: [&str; 2] = ["ipd", "netd"];
/// The programs that serve every domain by design, exempt from the check.
const EXEMPT: [&str; 2] = ["steward", "sshd"];

/// A label set, sorted, so that two spellings of one set compare equal.
type Set<'a> = Vec<&'a str>;

fn set(labels: &[String]) -> Set<'_> {
    let mut set: Set = labels.iter().map(String::as_str).collect();
    set.sort_unstable();
    set.dedup();
    set
}

/// Every domain a principal works in: its unlabelled set and each of its label sets.
fn principal_sets(m: &Manifest) -> Vec<(usize, Set<'_>)> {
    let mut out = Vec::new();
    for (i, p) in m.principals.iter().enumerate() {
        out.push((i, Set::new()));
        out.extend(p.label_sets.iter().map(|s| (i, set(&s.labels))));
    }
    out
}

fn exempt(s: &Server) -> bool { EXEMPT.contains(&s.program.as_str()) }

fn shared(s: &Server) -> bool { s.args.iter().any(|a| a.starts_with(BUCKETS_ARG)) }

/// Whether server `t` attaches the volume `s` verifies, through the badge `init` mints at `s`.
fn reads_through(m: &Manifest, s: &Server, t: &Server) -> bool {
    verified(m, s).is_some_and(|v| t.volume.as_ref() == Some(&v.name))
}

/// The label sets that use server `i`: its own, every server handed one of its endpoints or a
/// volume's range at it (an `fsd`, or a verified volume's `verityd`, on that `blkd`'s disk; an
/// `fsd` at its volume's `verityd`), and, if it is shared, every principal domain with its own
/// set.
fn users<'a>(m: &'a Manifest, i: usize) -> Vec<Set<'a>> {
    let s = &m.servers[i];
    let mut sets = alloc::vec![set(&s.labels)];
    let holds = |t: &Server| m.volumes.iter().any(|v| on_disk(m, v, s) && holds_range(m, v, t));
    for t in &m.servers {
        let range = (s.program == BLKD && holds(t)) || reads_through(m, s, t);
        if range || t.handed.iter().any(|h| s.receives.contains(&h.endpoint)) {
            sets.push(set(&t.labels));
        }
    }
    if shared(s) {
        let own = set(&s.labels);
        sets.extend(principal_sets(m).into_iter().map(|(_, set)| set).filter(|d| *d == own));
    }
    sets
}

fn differ(sets: &[Set]) -> bool { sets.windows(2).any(|w| w[0] != w[1]) }

fn refuse(at: String, sharing: Sharing) -> Result<(), Refusal> { Err(Refusal::Confined { at, sharing }) }

/// R34 for a manifest with `confined` set.
pub fn check(m: &Manifest) -> Result<(), Refusal> {
    // An endpoint: its receiver, every server handed it, and at a verifier's first, the server
    // attaching its volume.
    for (i, s) in m.servers.iter().enumerate().filter(|(_, s)| !exempt(s)) {
        for (k, e) in s.receives.iter().enumerate() {
            let mut sets = alloc::vec![set(&s.labels)];
            sets.extend(
                m.servers
                    .iter()
                    .filter(|t| {
                        t.handed.iter().any(|h| &h.endpoint == e) || (k == 0 && reads_through(m, s, t))
                    })
                    .map(|t| set(&t.labels)),
            );
            if differ(&sets) {
                return refuse(format!("servers[{i}].receives[{k}]"), Sharing::Endpoint);
            }
        }
    }
    // A volume: its own set, every server attaching it and every domain of a principal whose
    // home is on it. A labelled domain reading a shared unlabelled volume is one such case.
    for (i, v) in m.volumes.iter().enumerate() {
        let mut sets = alloc::vec![set(&v.labels)];
        sets.extend(m.servers.iter().filter(|s| s.volume.as_ref() == Some(&v.name)).map(|s| set(&s.labels)));
        for (p, domain) in principal_sets(m) {
            let home = m.principals[p].home.as_deref().and_then(|h| h.split_once(':'));
            if home.is_some_and(|(volume, _)| volume == v.name) {
                sets.push(domain);
            }
        }
        if differ(&sets) {
            return refuse(format!("volumes[{i}]"), Sharing::Volume);
        }
    }
    // A network instance: an `ipd` or `netd` used by two sets, and any labelled domain with a
    // network scope, since a labelled domain gets no `/net` at all.
    for (i, s) in m.servers.iter().enumerate().filter(|(_, s)| !exempt(s)) {
        if NETWORK.contains(&s.program.as_str()) && differ(&users(m, i)) {
            return refuse(format!("servers[{i}]"), Sharing::Network);
        }
    }
    for (p, domain) in principal_sets(m) {
        if !domain.is_empty() && !m.principals[p].net.is_empty() {
            return refuse(format!("principals[{p}].net"), Sharing::Network);
        }
    }
    // A device: the driver holding it is used by two sets.
    for (i, s) in m.servers.iter().enumerate().filter(|(_, s)| !exempt(s)) {
        if !s.devices.is_empty() && differ(&users(m, i)) {
            return refuse(format!("servers[{i}].devices"), Sharing::Device);
        }
    }
    // A server instance: any other server used by two sets, as a `blkd` with no devices whose
    // volumes carry a set of their own.
    for (i, _) in m.servers.iter().enumerate().filter(|(_, s)| !exempt(s)) {
        if differ(&users(m, i)) {
            return refuse(format!("servers[{i}]"), Sharing::Server);
        }
    }
    Ok(())
}
