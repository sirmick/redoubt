//! `ipd`'s arguments (NAMESPACES.md, What `ipd` serves; INIT.md: each server defines its own),
//! parsed strictly and all at once: an argument `ipd` does not understand, or one that breaks a
//! rule below, stops it (`BAD_ARGS`) before it serves anything.
//!
//! - `addr=A.B.C.D/LEN`: the box's address and its network, once. A unicast host address.
//! - `gateway=A.B.C.D`: the default route, at most once; unicast, on the network, not `addr`.
//! - `self=A.B.C.D/LEN`: more of the box's own addresses (on QEMU `10.0.2.0/24`), at most 8.
//! - `ingress=BADGE`: the badge `netd`'s frames arrive on, once. It has no `/net`.
//! - `scope=BADGE:RULE[,RULE...]`: what one root badge may do, at most 8 badges of at most 8 rules; a rule is
//!   `c:A.B.C.D/LEN:PORTS` or `l:PORTS`, `PORTS` being `P` or `LO-HI`.
//! - `buckets=N`: admission buckets, 1 to 32, at most once (default [`DEFAULT_BUCKETS`]).
//! - `limits=BADGE:INFLIGHT:STATE:SOCKETS`: caps for one scope badge's bucket in place of the defaults, at
//!   most 8.
//!
//! Numbers are decimal without leading zeros. Badges are nonzero, below 2^63 (the minted range),
//! and each appears once: the ingress badge is not a scope badge, and a `limits` badge must be
//! one.

use alloc::vec::Vec;

use crate::scope::{MAX_RULES, Ports, Prefix, Rule, Scope, ip, mask};

/// Buckets when `buckets=` is not given.
pub const DEFAULT_BUCKETS: u32 = 6;
/// The most `self=`, `scope=` and `limits=` arguments of each kind.
pub const MAX_EACH: usize = 8;

/// One `limits=` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit {
    pub badge: u64,
    pub in_flight: u32,
    pub state: u32,
    pub sockets: u32,
}

/// Everything `ipd` is told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub addr: u32,
    pub len: u8,
    pub gateway: Option<u32>,
    pub selfs: Vec<Prefix>,
    pub ingress: u64,
    pub scopes: Vec<(u64, Scope)>,
    pub buckets: u32,
    pub limits: Vec<Limit>,
}

/// The arguments are not ones `ipd` will run with; which one is a diagnostic for the tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadArgs(pub &'static str);

impl Config {
    pub fn scope_of(&self, badge: u64) -> Option<&Scope> {
        self.scopes.iter().find(|(b, _)| *b == badge).map(|(_, s)| s)
    }
}

/// Parses every argument; all or nothing.
pub fn parse<'a>(args: impl Iterator<Item = &'a str>) -> Result<Config, BadArgs> {
    let mut addr = None;
    let mut gateway = None;
    let mut selfs = Vec::new();
    let mut ingress = None;
    let mut scopes: Vec<(u64, Scope)> = Vec::new();
    let mut buckets = None;
    let mut limits: Vec<Limit> = Vec::new();
    for arg in args {
        let (key, value) = arg.split_once('=').ok_or(BadArgs("an argument without '='"))?;
        match key {
            "addr" => {
                once(&addr, "addr")?;
                let (a, len) = prefix_parts(value)?;
                addr = Some((a, len));
            }
            "gateway" => {
                once(&gateway, "gateway")?;
                gateway = Some(dotted(value)?);
            }
            "self" => {
                if selfs.len() == MAX_EACH {
                    return Err(BadArgs("too many self="));
                }
                let (a, len) = prefix_parts(value)?;
                selfs.push(Prefix::new(a, len).ok_or(BadArgs("self= is not a canonical prefix"))?);
            }
            "ingress" => {
                once(&ingress, "ingress")?;
                ingress = Some(badge(value)?);
            }
            "scope" => {
                if scopes.len() == MAX_EACH {
                    return Err(BadArgs("too many scope="));
                }
                let (b, rules) = value.split_once(':').ok_or(BadArgs("scope= without a badge"))?;
                let b = badge(b)?;
                if scopes.iter().any(|(other, _)| *other == b) {
                    return Err(BadArgs("a scope badge given twice"));
                }
                scopes.push((b, scope(rules)?));
            }
            "buckets" => {
                once(&buckets, "buckets")?;
                let n = number(value)?;
                if !(1..=32).contains(&n) {
                    return Err(BadArgs("buckets= must be 1 to 32"));
                }
                buckets = Some(n as u32);
            }
            "limits" => {
                if limits.len() == MAX_EACH {
                    return Err(BadArgs("too many limits="));
                }
                let parts: Vec<&str> = value.split(':').collect();
                let [b, in_flight, state, sockets] = parts[..] else {
                    return Err(BadArgs("limits= needs four fields"));
                };
                let small = |s: &str| number(s).ok().and_then(|n| u32::try_from(n).ok()).filter(|n| *n <= 64);
                let limit = Limit {
                    badge: badge(b)?,
                    in_flight: small(in_flight).ok_or(BadArgs("limits= in_flight"))?,
                    state: small(state).ok_or(BadArgs("limits= state"))?,
                    sockets: small(sockets).ok_or(BadArgs("limits= sockets"))?,
                };
                if limits.iter().any(|l| l.badge == limit.badge) {
                    return Err(BadArgs("a limits badge given twice"));
                }
                limits.push(limit);
            }
            _ => return Err(BadArgs("an argument ipd does not understand")),
        }
    }
    let (addr, len) = addr.ok_or(BadArgs("no addr="))?;
    if !(1..=30).contains(&len) || !unicast(addr) {
        return Err(BadArgs("addr= is not a unicast host address on a network"));
    }
    let network = addr & mask(len);
    let broadcast = network | !mask(len);
    if addr == network || addr == broadcast {
        return Err(BadArgs("addr= is its network's network or broadcast address"));
    }
    if let Some(g) = gateway {
        if !unicast(g) || g & mask(len) != network || g == addr || g == network || g == broadcast {
            return Err(BadArgs("gateway= is not a unicast address on the network"));
        }
    }
    let ingress = ingress.ok_or(BadArgs("no ingress="))?;
    if scopes.iter().any(|(b, _)| *b == ingress) {
        return Err(BadArgs("the ingress badge has a scope"));
    }
    if limits.iter().any(|l| !scopes.iter().any(|(b, _)| *b == l.badge)) {
        return Err(BadArgs("a limits badge has no scope"));
    }
    Ok(Config {
        addr,
        len,
        gateway,
        selfs,
        ingress,
        scopes,
        buckets: buckets.unwrap_or(DEFAULT_BUCKETS),
        limits,
    })
}

fn once<T>(slot: &Option<T>, what: &'static str) -> Result<(), BadArgs> {
    if slot.is_some() { Err(BadArgs(what)) } else { Ok(()) }
}

/// A decimal number without leading zeros.
fn number(s: &str) -> Result<u64, BadArgs> {
    let canonical = !s.is_empty()
        && s.len() <= 20
        && s.bytes().all(|b| b.is_ascii_digit())
        && (s == "0" || !s.starts_with('0'));
    s.parse().ok().filter(|_| canonical).ok_or(BadArgs("not a decimal number"))
}

/// A root badge: nonzero, below the minted range.
fn badge(s: &str) -> Result<u64, BadArgs> {
    number(s).ok().filter(|b| *b != 0 && *b < 1 << 63).ok_or(BadArgs("a badge must be 1 to 2^63 - 1"))
}

fn octet(s: &str) -> Option<u8> { number(s).ok().and_then(|n| u8::try_from(n).ok()) }

/// `a.b.c.d`.
fn dotted(s: &str) -> Result<u32, BadArgs> {
    let parts: Vec<&str> = s.split('.').collect();
    let [a, b, c, d] = parts[..] else { return Err(BadArgs("not an a.b.c.d address")) };
    match (octet(a), octet(b), octet(c), octet(d)) {
        (Some(a), Some(b), Some(c), Some(d)) => Ok(ip(a, b, c, d)),
        _ => Err(BadArgs("not an a.b.c.d address")),
    }
}

/// `a.b.c.d/len`, not yet checked for canonical form.
fn prefix_parts(s: &str) -> Result<(u32, u8), BadArgs> {
    let (a, len) = s.split_once('/').ok_or(BadArgs("a prefix without /LEN"))?;
    let len = number(len).ok().and_then(|n| u8::try_from(n).ok()).filter(|n| *n <= 32);
    Ok((dotted(a)?, len.ok_or(BadArgs("a prefix length must be 0 to 32"))?))
}

/// `P` or `LO-HI`.
fn ports(s: &str) -> Result<Ports, BadArgs> {
    let port = |p: &str| number(p).ok().and_then(|n| u16::try_from(n).ok()).filter(|p| *p != 0);
    let (lo, hi) = match s.split_once('-') {
        Some((lo, hi)) => (port(lo), port(hi)),
        None => (port(s), port(s)),
    };
    lo.zip(hi).and_then(|(lo, hi)| Ports::new(lo, hi)).ok_or(BadArgs("ports must be 1 to 65535, LO <= HI"))
}

/// `RULE[,RULE...]`.
fn scope(s: &str) -> Result<Scope, BadArgs> {
    let mut rules = Vec::new();
    for rule in s.split(',') {
        if rules.len() == MAX_RULES {
            return Err(BadArgs("too many rules"));
        }
        rules.push(if let Some(rest) = rule.strip_prefix("c:") {
            let (prefix, p) = rest.rsplit_once(':').ok_or(BadArgs("c: needs a prefix and ports"))?;
            let (a, len) = prefix_parts(prefix)?;
            Rule::Connect(Prefix::new(a, len).ok_or(BadArgs("a scope prefix is not canonical"))?, ports(p)?)
        } else if let Some(p) = rule.strip_prefix("l:") {
            Rule::Listen(ports(p)?)
        } else {
            return Err(BadArgs("a rule is c:... or l:..."));
        });
    }
    Scope::new(&rules).map_err(|_| BadArgs("too many rules"))
}

/// Not 0/8, loopback, multicast or class E.
fn unicast(a: u32) -> bool {
    let first = (a >> 24) as u8;
    first != 0 && first != 127 && first < 224
}

impl Config {
    /// The box's own addresses for this configuration (NAMESPACES.md).
    pub fn selfset(&self) -> crate::scope::SelfSet {
        crate::scope::SelfSet::new(self.addr, self.len, &self.selfs)
    }
}
