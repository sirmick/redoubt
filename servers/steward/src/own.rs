//! The steward's own lines (servers/steward.md, "The manifest lines"): after the core's, `init`
//! appends what only the server needs, each `init`'s check output naming only what the manifest
//! holds. The core knows labels and principals by id and name alone; these lines bind them to
//! the handles `init` handed the steward.
//!
//! - `label "NAME" id=N`: a label's name, for a login's label.
//! - `home "PRINCIPAL" handle=H path=/P quota=N`: the principal's home, at the server `init` handed the
//!   steward as the named handle `H`, rooted at the clean absolute path `/P`, holding at most `N` bytes (at
//!   least 1) whatever the principal's sessions.
//! - `vault "PRINCIPAL" labels=[..] handle=H`: the labelled volume of one label set the principal works
//!   under, at the server handed as `H`.
//! - `console "PRINCIPAL"`: the principal whose unlabelled session the steward opens on the UART, at its
//!   start and whenever that session ends; none without the line.
//! - `net "PRINCIPAL" PREFIX:PORTS ...`: the principal's network scope, in the manifest's form (`PORTS` a
//!   comma list, or `*` for every port), which goes to `ipd`'s `grant` as one connect rule per port (or one
//!   for every port), at most `ipd`'s eight.
//! - `confined`: the manifest sets `confined` (servers/init.md, "The confinement check"): a labelled
//!   session's home slot is left unbound, and a push is its way in ("Declassification and push").
//!
//! Strict as the core's lines are: each field once and no other, a name quoted with the trace
//! grammar's escapes, and a malformed line is a start failure naming its number.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_rt::path::is_clean_absolute;
use redoubt_rt::startup::valid_name;
use redoubt_rt::wire::ipd_scope::{self, RULE_BYTES};
use redoubt_steward::effect::Kind;
use redoubt_steward::manifest::{list, string, tokens, u64_of};

/// A principal's home.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    pub principal: String,
    pub handle: String,
    pub path: String,
    /// The bytes the home holds, for all the principal's sessions together.
    pub quota: u64,
}

/// A labelled volume one of a principal's label sets works on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vault {
    pub principal: String,
    /// Sorted, as the core's domains are.
    pub labels: Vec<u64>,
    pub handle: String,
}

/// A principal's network scope: its rules, each `PREFIX:PORTS`, and the same in `ipd`'s `grant`
/// encoding (servers/ipd.md, "The scope's encoding").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Net {
    pub principal: String,
    pub rules: Vec<String>,
    pub scope: Vec<u8>,
}

pub use redoubt_rt::wire::ipd_scope::MAX_RULES;

/// Everything the steward's own lines say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Own {
    pub labels: Vec<(String, u64)>,
    pub homes: Vec<Home>,
    pub vaults: Vec<Vault>,
    pub nets: Vec<Net>,
    pub console: Option<String>,
    pub confined: bool,
}

/// Whether `line` is one of the steward's own, rather than the core's.
pub fn is_own(line: &str) -> bool {
    line == "confined"
        || ["label ", "home ", "vault ", "net ", "console "].iter().any(|h| line.starts_with(h))
}

/// The `key=value` fields of `toks`, each of `keys` once, in `keys`' order, and no other.
fn fields<'a, const N: usize>(toks: &[&'a str], keys: [&str; N]) -> Result<[&'a str; N], String> {
    let mut out = [None; N];
    for t in toks {
        let (k, v) = t.split_once('=').ok_or(format!("`{t}` is not key=value"))?;
        let i = keys.iter().position(|x| *x == k).ok_or(format!("unknown field `{k}`"))?;
        if out[i].replace(v).is_some() {
            return Err(format!("a second `{k}=`"));
        }
    }
    let mut got = [""; N];
    for (i, k) in keys.iter().enumerate() {
        got[i] = out[i].ok_or(format!("no `{k}=`"))?;
    }
    Ok(got)
}

/// A name in the manifest's grammar (the startup block's: 1 to 64 bytes).
fn name(s: &str) -> Result<String, String> {
    let n = string(s)?;
    if valid_name(&n) { Ok(n) } else { Err(format!("{s} is not a name")) }
}

fn handle(s: &str) -> Result<String, String> {
    if valid_name(s) { Ok(s.into()) } else { Err(format!("`{s}` is not a handle name")) }
}

/// One rule, `PREFIX/LEN:PORTS` (IPv4, the ports a comma list of 1 to 65535 or `*`), appended
/// to `scope` as `ipd`'s connect rules: one per port, or one for every port.
fn rule(s: &str, scope: &mut Vec<[u8; RULE_BYTES]>) -> Result<String, String> {
    let bad = || format!("`{s}` is not PREFIX/LEN:PORTS");
    let (prefix, ports) = s.rsplit_once(':').ok_or_else(bad)?;
    let (addr, len) = prefix.split_once('/').ok_or_else(bad)?;
    let addr: core::net::Ipv4Addr = addr.parse().map_err(|_| bad())?;
    let len = u8::try_from(u64_of(len)?).ok().filter(|l| *l <= 32).ok_or_else(bad)?;
    let mut push =
        |lo: u16, hi: u16| scope.push(ipd_scope::rule(ipd_scope::CONNECT, addr.octets(), len, lo, hi));
    if ports == "*" {
        push(1, u16::MAX);
    } else {
        for p in ports.split(',') {
            let p = u16::try_from(u64_of(p)?).ok().filter(|p| *p != 0).ok_or_else(bad)?;
            push(p, p);
        }
    }
    Ok(s.into())
}

impl Own {
    /// Reads one line, which [`is_own`] said is the steward's.
    pub fn line(&mut self, line: &str) -> Result<(), String> {
        let toks = tokens(line)?;
        let (head, rest) = toks.split_first().ok_or("an empty line")?;
        if *head == "confined" {
            if !rest.is_empty() {
                return Err("`confined` stands alone".into());
            }
            if core::mem::replace(&mut self.confined, true) {
                return Err("a second `confined`".into());
            }
            return Ok(());
        }
        let (who, rest) = rest.split_first().ok_or("a line names its subject")?;
        match *head {
            "label" => {
                let [id] = fields(rest, ["id"])?;
                let label = name(who)?;
                if self.labels.iter().any(|(n, _)| *n == label) {
                    return Err(format!("a second label {label}"));
                }
                self.labels.push((label, u64_of(id)?));
            }
            "home" => {
                let [h, path, quota] = fields(rest, ["handle", "path", "quota"])?;
                if !is_clean_absolute(path) {
                    return Err(format!("`{path}` is not a clean absolute path"));
                }
                let quota = u64_of(quota)?;
                if quota == 0 {
                    return Err("a home's quota is at least 1 byte".into());
                }
                let principal = name(who)?;
                if self.homes.iter().any(|h| h.principal == principal) {
                    return Err(format!("a second home for {principal}"));
                }
                self.homes.push(Home { principal, handle: handle(h)?, path: path.into(), quota });
            }
            "vault" => {
                let [labels, h] = fields(rest, ["labels", "handle"])?;
                let mut labels = list(labels)?;
                labels.sort_unstable();
                labels.dedup();
                if labels.is_empty() {
                    return Err("a vault has labels".into());
                }
                let principal = name(who)?;
                if self.vaults.iter().any(|v| v.principal == principal && v.labels == labels) {
                    return Err(format!("a second vault for {principal} {labels:?}"));
                }
                self.vaults.push(Vault { principal, labels, handle: handle(h)? });
            }
            "console" => {
                if !rest.is_empty() {
                    return Err("`console` names one principal".into());
                }
                if self.console.replace(name(who)?).is_some() {
                    return Err("a second console".into());
                }
            }
            "net" => {
                let principal = name(who)?;
                if self.nets.iter().any(|n| n.principal == principal) {
                    return Err(format!("a second net for {principal}"));
                }
                let mut encoded = Vec::new();
                let rules = rest.iter().map(|r| rule(r, &mut encoded)).collect::<Result<Vec<_>, _>>()?;
                if encoded.is_empty() || encoded.len() > MAX_RULES {
                    return Err(format!("{} rules for {principal}: 1 to {MAX_RULES}", encoded.len()));
                }
                let scope = ipd_scope::encode(&encoded).unwrap_or_default();
                self.nets.push(Net { principal, rules, scope });
            }
            _ => return Err(format!("unknown line `{head}`")),
        }
        Ok(())
    }

    pub fn home(&self, principal: &str) -> Option<&Home> {
        self.homes.iter().find(|h| h.principal == principal)
    }

    pub fn vault(&self, principal: &str, labels: &[u64]) -> Option<&Vault> {
        self.vaults.iter().find(|v| v.principal == principal && v.labels == labels)
    }

    pub fn net(&self, principal: &str) -> Option<&Net> { self.nets.iter().find(|n| n.principal == principal) }
}

/// What a session's VM is told of itself, as launch arguments (servers/steward.md, "Authentication
/// and sessions"; userland/sessions.md, "What a session is told"): `principal=NAME`; `label=NAME:ID` for each
/// label of its set, in the set's order; `context=NAME` for a named context, none for the principal's
/// default one or the console's session; and `home=/PATH`, where slot 1 binds the principal's home, for a
/// labelled session as for a plain one, none for a principal with no home or for a labelled session in
/// a confined boot, whose home slot binds nothing ([`binding`]). Fixed for the VM's life, and
/// information only: what the session can reach is its connections, whatever these say.
pub fn session_args(own: &Own, principal: &str, labels: &[u64], context: Option<&str>) -> Vec<String> {
    // init's check gave every label of a set a name line; one without would have no name.
    let name = |id: &u64| own.labels.iter().find(|(_, l)| l == id).map_or("", |(n, _)| n.as_str());
    let confined = own.confined && !labels.is_empty();
    let labels = labels.iter().map(|id| format!("label={}:{id}", name(id)));
    let context = context.filter(|c| !c.is_empty()).map(|c| format!("context={c}"));
    let home = own.home(principal).filter(|_| !confined).map(|h| format!("home={}", h.path));
    core::iter::once(format!("principal={principal}")).chain(labels).chain(context).chain(home).collect()
}

/// What a lease's VM is told of itself: the sponsor it acts for, as `principal=NAME`, and where
/// its project root is bound, `home=/project`; no labels (a lease of this package is unlabelled)
/// and no context. Information only, as a session's.
pub fn lease_args(principal: &str) -> Vec<String> {
    alloc::vec![format!("principal={principal}"), format!("home={PROJECT_AT}")]
}

/// The system volume's server: the handle name the steward is handed it under and gives a
/// session's VM, which its `endpoint=` argument names.
pub const SYSTEM: &str = "erofsd:system";

/// How a session's slot is bound (servers/steward.md, "Two embedders and a reference").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    /// A fresh connection to the server the steward was handed as `server`, rooted at `root`.
    Fresh { server: String, root: String },
    /// A fresh connection minted, with no quota of its own, through the one connection the
    /// steward holds for `key`: minted once, at `server` rooted at `root` with `quota` bytes, and
    /// kept for the steward's life, so every session it binds shares that quota. The fresh one
    /// is rooted at `below`, a path under the kept one's root (empty for the root itself).
    Carved { key: String, server: String, root: String, quota: u64, below: String, below_quota: u64 },
    /// A connection `ipd` grants, narrowed to `scope` (its `grant` encoding).
    Grant { scope: Vec<u8> },
    /// The session's console: `sshd`'s channel, or a connection minted at `consoled`.
    Console,
}

/// The network's server: the handle name the steward is handed `ipd` under.
pub const IPD: &str = "ipd";

impl How {
    /// The shared server the connection is minted at, by the handle name the steward was handed
    /// it under; none for the console, which is no shared server of the sessions'.
    pub fn server(&self) -> Option<&str> {
        match self {
            How::Fresh { server, .. } | How::Carved { server, .. } => Some(server),
            How::Grant { .. } => Some(IPD),
            How::Console => None,
        }
    }
}

/// A slot's binding: how its connection is made, the namespace path it is bound at, and the
/// startup-block name it is handed under, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    pub how: How,
    pub at: Option<String>,
    pub name: Option<&'static str>,
}

/// Whose namespace a binding is for: a session's or a lease's, with labels or without.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Child {
    Session { labelled: bool },
    Lease { labelled: bool },
}

impl Child {
    /// A session's or a lease's, by the kind of the object the batch is for.
    pub fn of(kind: Kind, labels: &[u64]) -> Child {
        let labelled = !labels.is_empty();
        match kind {
            Kind::Lease => Child::Lease { labelled },
            _ => Child::Session { labelled },
        }
    }
}

/// The directory of the sponsor's home a lease works in (userland/agents.md, "Leases"), and the
/// path its namespace binds it at.
pub const PROJECT: &str = "project";
pub const PROJECT_AT: &str = "/project";

/// Whether `slot` can bind anything for `child`: for a session, the vault only for a labelled
/// one, the network only for an unlabelled one, the rest for both; for a lease, `/boot` (the
/// programs it may launch in its own sub-budgets), its project root and the system volume: no
/// vault slot, no network, no console (servers/steward.md, "Leases"). A labelled lease's
/// project root is its vault, bound at slot 1 ([`binding`]).
pub fn binds(child: Child, slot: u16) -> bool {
    match (child, slot) {
        (Child::Lease { .. }, s) => s == 0 || s == 1 || s == 5,
        (Child::Session { labelled }, 2) => labelled,
        (Child::Session { labelled }, 3) => !labelled,
        (Child::Session { .. }, s) => s < crate::SLOTS,
    }
}

/// The binding table, for `principal`'s `child` with `labels`, at `slot`: none where the slot
/// binds nothing for it, which makes no call and gives the child no entry. In a confined boot a
/// labelled session's home slot is one such: the home is a shared unlabelled volume, which a
/// labelled domain reads nothing of there (servers/init.md R34); its input is a push
/// (servers/steward.md, "Declassification and push"). `/boot` and the system volume, read-only
/// code the confinement check does not count for a labelled domain, stay bound.
pub fn binding(
    own: &Own,
    principal: &str,
    labels: &[u64],
    child: Child,
    slot: u16,
    lease_quota: u64,
) -> Option<Bound> {
    if !binds(child, slot) {
        return None;
    }
    let fresh = |server: &str, root: &str| How::Fresh { server: server.into(), root: root.into() };
    // An unlabelled lease's root is the sponsor's `project` directory, minted through the home's
    // one carve with a quota of its own, `lease_quota` (the manifest's `agent_quota`), carved
    // from the home's and given back at the lease's end: a root below the home with no quota
    // would hold nothing (walfsd.md, "Quotas"). A session's is the home itself, sharing the
    // home's quota. A labelled lease's is its label set's vault, whole: the vault is the task's
    // scope, with the vault's own quota, shared as a vault session shares it (R25 lets a
    // labelled agent touch nothing of the unlabelled home; userland/agents.md, "Leases").
    let (below, below_quota, at) = match child {
        Child::Lease { .. } => (PROJECT, lease_quota, PROJECT_AT),
        Child::Session { .. } => ("", 0, ""),
    };
    match slot {
        0 => Some(Bound { how: fresh("bootfsd", ""), at: Some("/boot".into()), name: Some("bootfsd") }),
        1 if child == (Child::Lease { labelled: true }) => own.vault(principal, labels).map(|v| Bound {
            how: fresh(&v.handle, ""),
            at: Some(PROJECT_AT.into()),
            name: None,
        }),
        1 if own.confined && child == (Child::Session { labelled: true }) => None,
        1 => own.home(principal).map(|h| Bound {
            how: How::Carved {
                key: format!("home {principal}"),
                server: h.handle.clone(),
                root: h.path.clone(),
                quota: h.quota,
                below: below.into(),
                below_quota,
            },
            at: Some(if at.is_empty() { h.path.clone() } else { at.into() }),
            name: None,
        }),
        2 => own.vault(principal, labels).map(|v| Bound {
            how: fresh(&v.handle, ""),
            at: Some("/vault".into()),
            name: None,
        }),
        3 => own.net(principal).map(|n| Bound {
            how: How::Grant { scope: n.scope.clone() },
            at: Some("/net".into()),
            name: None,
        }),
        4 => Some(Bound { how: How::Console, at: Some("/dev/cons".into()), name: None }),
        5 => Some(Bound { how: fresh(SYSTEM, ""), at: None, name: Some(SYSTEM) }),
        _ => None,
    }
}
