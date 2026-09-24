//! `ipd`'s arguments: any list of strings either parses and sizes, or is refused, without a
//! panic; and whatever is accepted keeps the rules `ipd` relies on (badges below 2^63, each
//! once; a unicast address on its network; canonical prefixes).
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_ipd::args::parse;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = core::str::from_utf8(data) else { return };
    let Ok(config) = parse(text.split(' ')) else { return };
    let mut badges = vec![config.ingress];
    badges.extend(config.scopes.iter().map(|(b, _)| *b));
    for (i, b) in badges.iter().enumerate() {
        assert!(*b != 0 && *b < 1 << 63);
        assert!(!badges[..i].contains(b), "a badge twice");
    }
    for l in &config.limits {
        assert!(config.scopes.iter().any(|(b, _)| *b == l.badge));
    }
    let mask = redoubt_ipd::scope::mask(config.len);
    assert!(config.addr & mask != config.addr || config.len == 32);
    for p in &config.selfs {
        assert_eq!(p.addr() & redoubt_ipd::scope::mask(p.len()), p.addr());
    }
    let _ = config.sizing();
});
