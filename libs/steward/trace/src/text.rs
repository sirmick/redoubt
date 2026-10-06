//! The trace text's lexical layer: a line's tokens, and the values a field holds
//! (servers/steward.md, "The trace encoding"). What the manifest lines share is the core's
//! (`redoubt_steward::manifest`), so the steward's arguments and a trace cannot drift; the rest is
//! the events'.

pub use redoubt_steward::manifest::{bytes, list, quote, show_list, string, tokens, u64_of};

/// A token `key=value`.
pub fn field<'a>(token: &'a str, key: &str) -> Option<&'a str> { token.strip_prefix(key)?.strip_prefix('=') }

pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

/// 64 hex digits.
pub fn hash(s: &str) -> Result<[u8; 32], String> {
    let bad = || format!("`{s}` is not 64 hex digits");
    if s.len() != 64 {
        return Err(bad());
    }
    let mut h = [0; 32];
    for (i, x) in h.iter_mut().enumerate() {
        *x = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(|_| bad())?;
    }
    Ok(h)
}
