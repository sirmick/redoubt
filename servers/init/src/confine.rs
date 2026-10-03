//! The confinement check (servers/init.md, "The confinement check"; R34 (confined placement)):
//! with `confined` set, two domains with differing label sets share nothing. Each sharing kind
//! is its own check with its own reason, in the page's order: endpoint, volume, network, device,
//! server instance.
//!
//! The domains are each `servers` entry, under its `labels`, and each label set a principal
//! works under, its unlabelled one included. The manifest does not route principals to servers
//! (the steward does), so a shared server, one sized with `buckets=N`, is used by every
//! principal's domains, as its bucket count assumes (servers/init.md, Sizing); a server is also
//! used by every server handed one of its endpoints, and `blkd` by every server attaching a
//! volume, whose range `init` mints at it, so a confined disk holds one label set's volumes. The
//! steward and `sshd` serve every domain by design and are exempt, by program name.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::check::{BLKD, BUCKETS_ARG};
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

/// The label sets that use server `i`: its own, every server handed one of its endpoints or a
/// volume's range at it (an `fsd` on `blkd`'s disk), and every principal's domains if it is
/// shared.
fn users<'a>(m: &'a Manifest, i: usize) -> Vec<Set<'a>> {
    let s = &m.servers[i];
    let mut sets = alloc::vec![set(&s.labels)];
    for t in &m.servers {
        let range = s.program == BLKD && t.volume.is_some();
        if range || t.handed.iter().any(|h| s.receives.contains(&h.endpoint)) {
            sets.push(set(&t.labels));
        }
    }
    if shared(s) {
        sets.extend(principal_sets(m).into_iter().map(|(_, set)| set));
    }
    sets
}

fn differ(sets: &[Set]) -> bool { sets.windows(2).any(|w| w[0] != w[1]) }

fn refuse(at: String, sharing: Sharing) -> Result<(), Refusal> { Err(Refusal::Confined { at, sharing }) }

/// R34 for a manifest with `confined` set.
pub fn check(m: &Manifest) -> Result<(), Refusal> {
    // An endpoint: its receiver and every server handed it.
    for (i, s) in m.servers.iter().enumerate().filter(|(_, s)| !exempt(s)) {
        for (k, e) in s.receives.iter().enumerate() {
            let mut sets = alloc::vec![set(&s.labels)];
            sets.extend(
                m.servers
                    .iter()
                    .filter(|t| t.handed.iter().any(|h| &h.endpoint == e))
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
    // A server instance: any other server used by two sets.
    for (i, _) in m.servers.iter().enumerate().filter(|(_, s)| !exempt(s)) {
        if differ(&users(m, i)) {
            return refuse(format!("servers[{i}]"), Sharing::Server);
        }
    }
    Ok(())
}
