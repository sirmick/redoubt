//! The matcher behind `binary:match` (docs/userland/beamlet.md, "OTP and Elixir unchanged"): a
//! search costs the haystack's length whatever the pattern count, and finds what a comparison of
//! every pattern at every byte finds.

use std::time::{Duration, Instant};

use beamlet_vm::bif::Matcher;

/// The search as it was before the matcher: every pattern compared at every byte, the longest
/// match at the leftmost position. The reference the matcher is held to.
fn every_pattern_at_every_byte(
    hay: &[u8],
    pats: &[Vec<u8>],
    from: usize,
    until: usize,
) -> Option<(usize, usize)> {
    (from..until).find_map(|i| {
        pats.iter()
            .filter(|p| i + p.len() <= until && hay[i..i + p.len()] == p[..])
            .map(|p| p.len())
            .max()
            .map(|len| (i, len))
    })
}

fn utf8(cp: u32) -> Vec<u8> { char::from_u32(cp).expect("a code point").to_string().into_bytes() }

/// The shell's terminal guard's patterns, every control character's UTF-8 (74), and 14 more
/// format characters, 88 in all; the first is one pattern and the first eight are eight.
fn guard_patterns() -> Vec<Vec<u8>> {
    let controls = (0..=0x1F).chain(0x7F..=0x9F).chain(0x202A..=0x202E).chain(0x2066..=0x2069);
    let formats =
        (0x200B..=0x200F).chain([0x2028, 0x2029, 0xFEFF, 0x061C, 0xFFF9, 0xFFFA, 0xFFFB, 0x180E, 0xAD]);
    let pats: Vec<Vec<u8>> = controls.chain(formats).map(utf8).collect();
    assert_eq!(pats.len(), 88);
    pats
}

/// A line of 64 KiB of printable ASCII (what `shell-output-rate` draws), which no pattern matches:
/// a newline would, being a control character.
fn page() -> Vec<u8> {
    let sentence = "The quick brown fox jumps over the lazy dog; 0123456789 and [more] {yet} (end)!! ";
    sentence.repeat((64 << 10) / sentence.len()).into_bytes()
}

/// The least of five runs of `f`.
fn least(mut f: impl FnMut()) -> Duration {
    (0..5)
        .map(|_| {
            let started = Instant::now();
            f();
            started.elapsed()
        })
        .min()
        .expect("five runs")
}

/// With 70 patterns the shell's guard cost the printed-text path what it did; at 88 the output
/// rate fell 5–8 %, the old search comparing every pattern at every byte. The matcher's time
/// over a page is flat in the pattern count: 88 patterns cost at most three times one.
#[test]
fn the_search_is_flat_in_the_pattern_count() {
    let page = page();
    let pats = guard_patterns();
    let mut report = String::new();
    let mut times = Vec::new();
    for n in [1, 8, 88] {
        let subset: Vec<Vec<u8>> = pats[..n].to_vec();
        let m = Matcher::new(subset.clone());
        let compiled = least(|| assert_eq!(m.find(&page, 0, page.len()), None));
        let built = least(|| assert_eq!(Matcher::new(subset.clone()).find(&page, 0, page.len()), None));
        let before = least(|| assert_eq!(every_pattern_at_every_byte(&page, &subset, 0, page.len()), None));
        let per_byte = |t: Duration| t.as_nanos() as f64 / page.len() as f64;
        report += &format!(
            "{n:>2} patterns over {} bytes: compiled {:.2} ns/byte, built per call {:.2}, before {:.2}\n",
            page.len(),
            per_byte(compiled),
            per_byte(built),
            per_byte(before)
        );
        times.push(compiled);
    }
    print!("{report}");
    let (one, many) = (times[0], times[2]);
    assert!(
        many <= one * 3 + Duration::from_micros(50),
        "88 patterns cost {many:?} where one costs {one:?}\n{report}"
    );
}

/// Over random haystacks, pattern sets and scopes, the matcher finds exactly what comparing every
/// pattern at every byte finds: the same position and the same length, or nothing.
#[test]
fn the_matcher_finds_what_every_pattern_at_every_byte_finds() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((seed >> 33) as usize) % n
    };
    for _ in 0..2_000 {
        let hay: Vec<u8> = (0..next(24)).map(|_| b"abcd"[next(4)]).collect();
        let pats: Vec<Vec<u8>> =
            (0..1 + next(5)).map(|_| (0..1 + next(4)).map(|_| b"abcd"[next(4)]).collect()).collect();
        let m = Matcher::new(pats.clone());
        for from in 0..=hay.len() {
            for until in from..=hay.len() {
                assert_eq!(
                    m.find(&hay, from, until),
                    every_pattern_at_every_byte(&hay, &pats, from, until),
                    "{:?} in {:?} [{from}, {until})",
                    pats.iter().map(|p| String::from_utf8_lossy(p).into_owned()).collect::<Vec<_>>(),
                    String::from_utf8_lossy(&hay)
                );
            }
        }
    }
}
