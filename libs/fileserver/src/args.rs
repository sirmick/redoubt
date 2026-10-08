//! The arguments `init` passes a volume server (servers/init.md, "The boot manifest"): the name
//! of the endpoint it receives on and the volume's label set. The rules are the manifest's, and
//! a server that misreads them serves under the wrong endpoint or labels, so they are written
//! once: `littlefsd`, `walfsd` and `erofsd` take [`parse_args`] whole, and `verityd`, whose
//! arguments also name what it checks the volume against, takes [`label_set`] and [`number`].
//! `buckets=N` is the skeleton's own argument (`redoubt_rt::server::buckets`), parsed before these
//! see the rest (`redoubt_rt::server::own_args`).

use alloc::vec::Vec;

use redoubt_rt::abi::MAX_LABELS;
use redoubt_rt::startup::valid_name;

/// Why the arguments were refused: the server then does not start, rather than guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadArgs;

/// What a volume server's arguments other than `buckets=` say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args<'a> {
    /// The manifest name of the endpoint it receives on (`littlefsd:data`, `erofsd:system`): its
    /// startup block holds that endpoint under this name.
    pub endpoint: &'a str,
    /// The volume's label set; empty when `labels=` is absent.
    pub labels: Vec<u64>,
}

/// A decimal number without leading zeros, the form `init` writes; `0` is itself.
pub fn number(s: &str) -> Result<u64, BadArgs> {
    let canonical =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    s.parse().ok().filter(|_| canonical).ok_or(BadArgs)
}

/// The value of `labels=`: IDs separated by commas, each a [`number`], distinct, at most
/// `MAX_LABELS` of them, in the order given. An empty value is refused: an absent `labels=` is
/// the empty set, an empty one a mistake.
pub fn label_set(list: &str) -> Result<Vec<u64>, BadArgs> {
    let mut set = Vec::new();
    for id in list.split(',') {
        let id = number(id)?;
        if set.contains(&id) || set.len() >= MAX_LABELS {
            return Err(BadArgs);
        }
        set.try_reserve(1).map_err(|_| BadArgs)?;
        set.push(id);
    }
    Ok(set)
}

/// The arguments other than `buckets=`: `endpoint=NAME` exactly once, a name under the manifest's
/// rule, never defaulted; and `labels=ID[,ID...]` at most once ([`label_set`]), absent for an
/// empty set. Anything else is refused, so the server never serves under an endpoint or labels it
/// misread.
pub fn parse_args<'a>(args: impl Iterator<Item = &'a str>) -> Result<Args<'a>, BadArgs> {
    let (mut endpoint, mut labels) = (None, None);
    for arg in args {
        if let Some(name) = arg.strip_prefix("endpoint=") {
            if endpoint.is_some() || !valid_name(name) {
                return Err(BadArgs);
            }
            endpoint = Some(name);
            continue;
        }
        let list = arg.strip_prefix("labels=").ok_or(BadArgs)?;
        if labels.is_some() {
            return Err(BadArgs);
        }
        labels = Some(label_set(list)?);
    }
    Ok(Args { endpoint: endpoint.ok_or(BadArgs)?, labels: labels.unwrap_or_default() })
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
