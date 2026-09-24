//! The socket capability (NAMESPACES.md, The network tree; answer 174): **IP prefixes and ports
//! only**, and never the box's own addresses.
//!
//! A [`Scope`] is at most [`MAX_RULES`] rules, each a **connect** rule (an IPv4 prefix and a port
//! range) or a **listen** rule (a port range). A connection may connect to an address and port
//! only if some connect rule contains both, and listen on a port only if some listen rule
//! contains it. Before any scope is looked at, the [`SelfSet`] refuses every address of the box
//! itself, so a manifest that wrongly scoped `0.0.0.0/0` still cannot reach the box.
//!
//! A scope is only ever narrowed ([`Scope::narrows`]): every rule a grant asks for must lie inside
//! one rule of the caller's own, of the same kind, prefix within prefix, ports within ports.

use alloc::vec::Vec;

/// The most rules one scope holds.
pub const MAX_RULES: usize = 8;

/// An IPv4 prefix: an address (as a big-endian `u32`, so `10.0.0.0` is `0x0a00_0000`) and a
/// length of 0 to 32. Canonical: no bit is set after the length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefix {
    addr: u32,
    len: u8,
}

impl Prefix {
    /// The prefix, if `len` is at most 32 and `addr` has no bit set past it.
    pub fn new(addr: u32, len: u8) -> Option<Prefix> {
        (len <= 32 && addr & !mask(len) == 0).then_some(Prefix { addr, len })
    }

    /// The prefix `addr/len` names, with the host bits cleared: for addresses the box derives
    /// from its own configuration (its network), never for a scope a client wrote.
    pub fn covering(addr: u32, len: u8) -> Prefix {
        let len = len.min(32);
        Prefix { addr: addr & mask(len), len }
    }

    pub fn addr(&self) -> u32 { self.addr }

    pub fn len(&self) -> u8 { self.len }

    pub fn contains(&self, addr: u32) -> bool { addr & mask(self.len) == self.addr }

    /// Whether every address of `other` is in `self`.
    pub fn covers(&self, other: &Prefix) -> bool { other.len >= self.len && self.contains(other.addr) }
}

/// The network mask of a prefix of `len` bits.
pub const fn mask(len: u8) -> u32 { if len == 0 { 0 } else { u32::MAX << (32 - len as u32) } }

/// An inclusive port range, `lo` at most `hi`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ports {
    lo: u16,
    hi: u16,
}

impl Ports {
    pub fn new(lo: u16, hi: u16) -> Option<Ports> { (lo <= hi).then_some(Ports { lo, hi }) }

    pub fn contains(&self, port: u16) -> bool { (self.lo..=self.hi).contains(&port) }

    pub fn covers(&self, other: &Ports) -> bool { self.lo <= other.lo && other.hi <= self.hi }

    pub fn lo(&self) -> u16 { self.lo }

    pub fn hi(&self) -> u16 { self.hi }
}

/// One rule of a scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Connect(Prefix, Ports),
    Listen(Ports),
}

impl Rule {
    /// Whether `self` asks for nothing `held` does not already allow.
    fn within(&self, held: &Rule) -> bool {
        match (self, held) {
            (Rule::Connect(p, ports), Rule::Connect(hp, hports)) => hp.covers(p) && hports.covers(ports),
            (Rule::Listen(ports), Rule::Listen(hports)) => hports.covers(ports),
            _ => false,
        }
    }
}

/// A connection's scope: at most [`MAX_RULES`] rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    rules: Vec<Rule>,
}

/// A scope that does not decode, or has too many rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadScope;

impl Scope {
    pub fn new(rules: &[Rule]) -> Result<Scope, BadScope> {
        if rules.len() > MAX_RULES {
            return Err(BadScope);
        }
        let mut owned = Vec::new();
        owned.try_reserve(rules.len()).map_err(|_| BadScope)?;
        owned.extend_from_slice(rules);
        Ok(Scope { rules: owned })
    }

    pub fn rules(&self) -> &[Rule] { &self.rules }

    pub fn permits_connect(&self, addr: u32, port: u16) -> bool {
        self.rules
            .iter()
            .any(|r| matches!(r, Rule::Connect(p, ports) if p.contains(addr) && ports.contains(port)))
    }

    pub fn permits_listen(&self, port: u16) -> bool {
        self.rules.iter().any(|r| matches!(r, Rule::Listen(ports) if ports.contains(port)))
    }

    /// Whether `requested` is no wider than `self`: each of its rules lies inside one of ours.
    /// So a grant can never widen a scope, and never add `listen` to one without it.
    pub fn narrows(&self, requested: &Scope) -> bool {
        requested.rules.iter().all(|r| self.rules.iter().any(|held| r.within(held)))
    }

    /// The `bytes` layout of `ipd`'s `grant` (NAMESPACES.md): `count: u8`, then per rule
    /// `kind: u8` (1 connect, 2 listen), `addr: bytes[4]` (network order; zero for listen),
    /// `len: u8` (zero for listen), `lo: u16`, `hi: u16`, little-endian as all of WIRE.md.
    pub fn decode(bytes: &[u8]) -> Result<Scope, BadScope> {
        let (&count, mut rest) = bytes.split_first().ok_or(BadScope)?;
        if usize::from(count) > MAX_RULES || rest.len() != usize::from(count) * RULE_BYTES {
            return Err(BadScope);
        }
        let mut rules = Vec::new();
        rules.try_reserve(usize::from(count)).map_err(|_| BadScope)?;
        while let Some((rule, tail)) = rest.split_first_chunk::<RULE_BYTES>() {
            let addr = u32::from_be_bytes([rule[1], rule[2], rule[3], rule[4]]);
            let len = rule[5];
            let ports =
                Ports::new(u16::from_le_bytes([rule[6], rule[7]]), u16::from_le_bytes([rule[8], rule[9]]))
                    .ok_or(BadScope)?;
            rules.push(match rule[0] {
                1 => Rule::Connect(Prefix::new(addr, len).ok_or(BadScope)?, ports),
                2 if addr == 0 && len == 0 => Rule::Listen(ports),
                _ => return Err(BadScope),
            });
            rest = tail;
        }
        Ok(Scope { rules })
    }

    /// The same layout, for a client building a `grant` (and the tests).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.rules.len() * RULE_BYTES);
        out.push(self.rules.len() as u8);
        for rule in &self.rules {
            let (kind, addr, len, ports) = match rule {
                Rule::Connect(p, ports) => (1u8, p.addr, p.len, ports),
                Rule::Listen(ports) => (2u8, 0, 0, ports),
            };
            out.push(kind);
            out.extend_from_slice(&addr.to_be_bytes());
            out.push(len);
            out.extend_from_slice(&ports.lo.to_le_bytes());
            out.extend_from_slice(&ports.hi.to_le_bytes());
        }
        out
    }
}

/// Bytes of one encoded rule.
pub const RULE_BYTES: usize = 10;

/// The box's own addresses, refused to every scope (NAMESPACES.md; CAPABILITIES.md, approvals):
/// `ipd`'s address, its network's network and broadcast addresses, the limited broadcast, the
/// loopback and "this host" networks, multicast and the reserved class E, and every `self=`
/// prefix the manifest lists (on QEMU, `10.0.2.0/24`, which slirp maps to the host).
#[derive(Clone, Debug, Default)]
pub struct SelfSet {
    prefixes: Vec<Prefix>,
}

impl SelfSet {
    /// The set for a box at `addr/len` with the manifest's `extra` prefixes.
    pub fn new(addr: u32, len: u8, extra: &[Prefix]) -> SelfSet {
        let network = Prefix::covering(addr, len);
        let broadcast = network.addr | !mask(network.len);
        let fixed = [
            Prefix::covering(addr, 32),
            Prefix::covering(network.addr, 32),
            Prefix::covering(broadcast, 32),
            Prefix::covering(u32::MAX, 32),
            Prefix::covering(0x7f00_0000, 8),
            Prefix::covering(0, 8),
            Prefix::covering(0xe000_0000, 4),
            Prefix::covering(0xf000_0000, 4),
        ];
        let mut prefixes = Vec::with_capacity(fixed.len() + extra.len());
        prefixes.extend_from_slice(&fixed);
        prefixes.extend_from_slice(extra);
        SelfSet { prefixes }
    }

    pub fn contains(&self, addr: u32) -> bool { self.prefixes.iter().any(|p| p.contains(addr)) }

    pub fn prefixes(&self) -> &[Prefix] { &self.prefixes }
}

/// Whether a packet claiming to come from `addr` claims to come from the box itself, or from
/// nowhere: `ipd`'s address, loopback or "this host". Such inbound TCP is dropped. The rest of the
/// self set is not: QEMU's forwarded connections arrive from `10.0.2.2`.
pub fn martian_source(addr: u32, own: u32) -> bool {
    addr == own || Prefix::covering(0x7f00_0000, 8).contains(addr) || Prefix::covering(0, 8).contains(addr)
}

/// `a.b.c.d` as a `u32`.
pub const fn ip(a: u8, b: u8, c: u8, d: u8) -> u32 { u32::from_be_bytes([a, b, c, d]) }
