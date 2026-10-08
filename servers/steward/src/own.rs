//! The steward's own lines (servers/steward.md, "The manifest lines"): after the core's, `init`
//! appends what only the server needs, each `init`'s check output naming only what the manifest
//! holds. The core knows labels and principals by id and name alone; these lines bind them to
//! the handles `init` handed the steward.
//!
//! - `label "NAME" id=N`: a label's name, for a login's label.
//! - `home "PRINCIPAL" handle=H path=/P`: the principal's home, at the server `init` handed the steward as
//!   the named handle `H`, rooted at the clean absolute path `/P`.
//! - `vault "PRINCIPAL" labels=[..] handle=H`: the labelled volume of one label set the principal works
//!   under, at the server handed as `H`.
//! - `console "PRINCIPAL"`: the principal whose unlabelled session the steward opens on the UART, at its
//!   start and whenever that session ends; none without the line.
//! - `net "PRINCIPAL" PREFIX:PORTS ...`: the principal's network scope, in the manifest's form (`PORTS` a
//!   comma list, or `*` for every port), which goes to `ipd`'s `grant` as one connect rule per port (or one
//!   for every port), at most `ipd`'s eight.
//!
//! Strict as the core's lines are: each field once and no other, a name quoted with the trace
//! grammar's escapes, and a malformed line is a start failure naming its number.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_rt::path::is_clean_absolute;
use redoubt_rt::startup::valid_name;
use redoubt_rt::wire::ipd_scope::{self, RULE_BYTES};
use redoubt_steward::manifest::{list, string, tokens, u64_of};

/// A principal's home.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    pub principal: String,
    pub handle: String,
    pub path: String,
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
}

/// Whether `line` is one of the steward's own, rather than the core's.
pub fn is_own(line: &str) -> bool {
    ["label ", "home ", "vault ", "net ", "console "].iter().any(|h| line.starts_with(h))
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
                let [h, path] = fields(rest, ["handle", "path"])?;
                if !is_clean_absolute(path) {
                    return Err(format!("`{path}` is not a clean absolute path"));
                }
                let principal = name(who)?;
                if self.homes.iter().any(|h| h.principal == principal) {
                    return Err(format!("a second home for {principal}"));
                }
                self.homes.push(Home { principal, handle: handle(h)?, path: path.into() });
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

/// The system volume's server: the handle name the steward is handed it under and gives a
/// session's VM, which its `endpoint=` argument names.
pub const SYSTEM: &str = "erofsd:system";

/// How a session's slot is bound (servers/steward.md, "Two embedders and a reference").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    /// A fresh connection to the server the steward was handed as `server`, rooted at `root`.
    Fresh { server: String, root: String },
    /// A connection `ipd` grants, narrowed to `scope` (its `grant` encoding).
    Grant { scope: Vec<u8> },
    /// The session's console: `sshd`'s channel, or a connection minted at `consoled`.
    Console,
}

/// A slot's binding: how its connection is made, the namespace path it is bound at, and the
/// startup-block name it is handed under, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    pub how: How,
    pub at: Option<String>,
    pub name: Option<&'static str>,
}

/// Whether `slot` can bind anything for a session with labels (`labelled`) or without: the vault
/// only for a labelled one, the network only for an unlabelled one, the rest for both.
pub fn binds(labelled: bool, slot: u16) -> bool {
    match slot {
        2 => labelled,
        3 => !labelled,
        s => s < crate::SLOTS,
    }
}

/// The binding table, for `principal`'s session with `labels`, at `slot`: none where the slot
/// binds nothing for that session, which makes no call and gives the child no entry.
pub fn binding(own: &Own, principal: &str, labels: &[u64], slot: u16) -> Option<Bound> {
    if !binds(!labels.is_empty(), slot) {
        return None;
    }
    let fresh = |server: &str, root: &str| How::Fresh { server: server.into(), root: root.into() };
    match slot {
        0 => Some(Bound { how: fresh("bootfsd", ""), at: Some("/boot".into()), name: Some("bootfsd") }),
        1 => own.home(principal).map(|h| Bound {
            how: fresh(&h.handle, &h.path),
            at: Some(h.path.clone()),
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
