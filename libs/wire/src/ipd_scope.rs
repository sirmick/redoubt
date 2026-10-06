//! A scope as `ipd`'s `grant` carries it in its `scope: bytes` (servers/ipd.md, "The scope's
//! encoding"): `count: u8`, then per rule `kind: u8` ([`CONNECT`] or [`LISTEN`]), `addr: bytes[4]`
//! (network order; zero for listen), `len: u8` (zero for listen), `lo: u16`, `hi: u16`,
//! little-endian as all of servers/wire.md. `ipd` decodes it into its own types; a client builds
//! it here.

use alloc::vec::Vec;

/// The most rules one scope holds.
pub const MAX_RULES: usize = 8;
/// Bytes of one encoded rule.
pub const RULE_BYTES: usize = 10;
/// A connect rule's kind: an IPv4 prefix and a port range.
pub const CONNECT: u8 = 1;
/// A listen rule's kind: a port range.
pub const LISTEN: u8 = 2;

/// One rule's bytes.
pub fn rule(kind: u8, addr: [u8; 4], len: u8, lo: u16, hi: u16) -> [u8; RULE_BYTES] {
    let mut r = [0u8; RULE_BYTES];
    r[0] = kind;
    r[1..5].copy_from_slice(&addr);
    r[5] = len;
    r[6..8].copy_from_slice(&lo.to_le_bytes());
    r[8..10].copy_from_slice(&hi.to_le_bytes());
    r
}

/// A scope's bytes: the count, then the rules; `None` past [`MAX_RULES`].
pub fn encode(rules: &[[u8; RULE_BYTES]]) -> Option<Vec<u8>> {
    if rules.len() > MAX_RULES {
        return None;
    }
    let mut out = Vec::with_capacity(1 + rules.len() * RULE_BYTES);
    out.push(rules.len() as u8);
    for r in rules {
        out.extend_from_slice(r);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_is_its_count_and_its_rules_and_holds_at_most_eight() {
        let r = rule(CONNECT, [10, 0, 0, 0], 8, 22, 443);
        assert_eq!(r, [1, 10, 0, 0, 0, 8, 22, 0, 0xbb, 1]);
        assert_eq!(encode(&[r, rule(LISTEN, [0; 4], 0, 22, 22)]).unwrap().len(), 1 + 2 * RULE_BYTES);
        assert!(encode(&[r; MAX_RULES]).is_some());
        assert!(encode(&[r; MAX_RULES + 1]).is_none());
    }
}
