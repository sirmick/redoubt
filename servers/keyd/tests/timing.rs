//! The timing check for the signing path (servers/keyd.md R45; TENETS.md, "Side channels":
//! assume the attacker has a perfect clock).
//!
//! **What it measures.** A fixed-versus-random test, as `dudect` does it. Two classes of
//! sample, interleaved by a deterministic coin so that any drift in the machine falls on both:
//! class A signs with one fixed key, class B with a key drawn from a pool, and both sign the
//! same message, so the only thing that differs between the classes is the secret. Each sample
//! is one signature, timed on its own. The statistic is each class's 10th percentile — low
//! enough to sit under the scheduler noise, which only ever adds time, and far enough from the
//! minimum to be steady — and the verdict is how far apart the two percentiles are.
//!
//! **What it proves, and what it does not.** It resolves a difference of a few per cent of one
//! signature between the two classes. [`LEAK`] is the control: a signer that does that much
//! extra work for class B and none for class A, which the same statistic must flag in the same
//! run. So the threshold is not a guess — the control says what this machine can see, and the
//! real signer has to be under it. It does not prove constant time, and says nothing about
//! microarchitectural channels, which TENETS.md puts out of scope for the software. The claim
//! that the path is constant time rests on reading the code (`keys::Key::sign` says what was
//! read); this is what would notice if that stopped being true.
//!
//! **Release only.** A `cargo test` build of `ed25519-compact` is six times slower and its
//! noise is a different shape, so the test is ignored there, with the reason, rather than
//! shipping one that cannot fail:
//!
//! ```text
//! cargo test --release -p redoubt-keyd --test timing
//! ```

use std::time::Instant;

use ed25519_compact::sha512;
use redoubt_keyd::keys::Keys;

/// Samples per class. One signature each, so this is also the work: 2 × [`SAMPLES`] signatures.
const SAMPLES: usize = 6000;
/// Keys class B draws from: one short of [`redoubt_keyd::keys::MAX_KEYS`], which is what a
/// `Keys` will hold, with the fixed key making up the set. They are small enough together that
/// every one stays in the first-level cache, so the classes differ by their secrets and not by
/// where the secrets live.
const POOL: usize = redoubt_keyd::keys::MAX_KEYS - 1;
/// Samples dropped before the classes are compared, so the first calls do not pay for page
/// faults and an untrained branch predictor on one class's behalf.
const WARMUP: usize = 500;

/// The percentile compared, in per cent: low enough to sit under scheduler noise, which only
/// ever adds, and far enough from the minimum to be steady.
const PERCENTILE: usize = 10;

/// How far apart the two classes may be, as a fraction of one signature.
const SAME: f64 = 0.02;
/// What the control leaks: a fraction of one signature, for class B only. The check must flag
/// it, or this run could not have seen a leak of that size and proves nothing.
const LEAK: f64 = 0.05;

fn hex(seed: &[u8; 32]) -> String { seed.iter().map(|b| format!("{b:02x}")).collect() }

/// `POOL` + 1 keys: the fixed one is first, the pool follows.
fn keys() -> Keys {
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let args: Vec<String> = (0..=POOL)
        .map(|i| {
            let mut seed = [0u8; 32];
            for byte in seed.iter_mut() {
                *byte = next() as u8;
            }
            format!("k{i},audit,{}", hex(&seed))
        })
        .collect();
    Keys::from_args(args.iter().map(String::as_str)).unwrap()
}

/// A deterministic coin and index, so a failure reproduces exactly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Runs the two classes interleaved and returns their [`PERCENTILE`] times, in nanoseconds.
/// `sign` is given the key index to use: 0 for class A, 1..=POOL for class B.
fn classes(mut sign: impl FnMut(usize)) -> (f64, f64) {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut a: Vec<u64> = Vec::with_capacity(SAMPLES);
    let mut b: Vec<u64> = Vec::with_capacity(SAMPLES);
    let mut taken = 0;
    while a.len() < SAMPLES || b.len() < SAMPLES {
        let r = rng.next();
        // Class B unless its samples are in; the coin keeps the two interleaved, so a slow
        // patch of the machine lands on both.
        let class_b = (r & 1 == 1 || a.len() == SAMPLES) && b.len() < SAMPLES;
        let index = if class_b { 1 + (r >> 8) as usize % POOL } else { 0 };
        let start = Instant::now();
        sign(index);
        let elapsed = start.elapsed().as_nanos() as u64;
        taken += 1;
        if taken > WARMUP {
            if class_b {
                b.push(elapsed);
            } else {
                a.push(elapsed);
            }
        }
    }
    (percentile(&mut a), percentile(&mut b))
}

fn percentile(samples: &mut [u64]) -> f64 {
    samples.sort_unstable();
    samples[samples.len() * PERCENTILE / 100] as f64
}

/// How far apart the classes are, as a fraction of one signature.
fn apart(a: f64, b: f64) -> f64 { (a - b).abs() / a.min(b) }

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "needs the optimised build the box ships: cargo test --release -p redoubt-keyd --test timing"
)]
fn signing_takes_the_same_time_whatever_the_key() {
    let keys = keys();
    let message = [0x5au8; 64];

    // The control first: a signer that does `LEAK` of a signature more for class B. If the
    // check cannot see that, this run resolves nothing and says so rather than passing.
    let one = {
        let (a, b) = classes(|i| {
            std::hint::black_box(keys.get(i).unwrap().sign(&message));
        });
        a.min(b)
    };
    // How many SHA-512 calls that fraction of a signature is worth.
    let hash_cost = {
        let start = Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(sha512::Hash::hash(message));
        }
        start.elapsed().as_nanos() as f64 / 1000.0
    };
    let extra = ((one * LEAK) / hash_cost).round().max(1.0) as usize;
    let (a, b) = classes(|i| {
        std::hint::black_box(keys.get(i).unwrap().sign(&message));
        if i != 0 {
            for _ in 0..extra {
                std::hint::black_box(sha512::Hash::hash(message));
            }
        }
    });
    let control = apart(a, b);
    assert!(
        control > SAME,
        "a leak of {:.0}% of a signature was not seen (measured {:.4}, threshold {SAME}): this \
         machine is too noisy for the measurement to mean anything",
        LEAK * 100.0,
        control
    );

    // Now the real signer, measured the same way.
    let (a, b) = classes(|i| {
        std::hint::black_box(keys.get(i).unwrap().sign(&message));
    });
    let real = apart(a, b);
    assert!(
        real < SAME,
        "signing time depends on the key: the classes are {:.4} of a signature apart \
         (threshold {SAME}; the control's {:.0}% leak measured {:.4}). Fixed key {a} ns, random \
         keys {b} ns at the {PERCENTILE}th percentile.",
        real,
        LEAK * 100.0,
        control
    );
}

/// The same, with the secret being the nonce rather than the key. Ed25519 derives the nonce
/// from the key's prefix and the message, and the scalar multiplication uses it, so different
/// messages of the same length exercise different secret scalars under one key.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "needs the optimised build the box ships: cargo test --release -p redoubt-keyd --test timing"
)]
fn signing_takes_the_same_time_whatever_the_nonce() {
    let keys = keys();
    let key = keys.get(0).unwrap();
    let fixed = [0x5au8; 64];
    let messages: Vec<[u8; 64]> = (0..=POOL)
        .map(|i| {
            let mut m = [0u8; 64];
            for (j, byte) in m.iter_mut().enumerate() {
                *byte = (i * 37 + j * 11) as u8;
            }
            m
        })
        .collect();
    let (a, b) = classes(|i| {
        let message = if i == 0 { &fixed } else { &messages[i] };
        std::hint::black_box(key.sign(message));
    });
    let real = apart(a, b);
    assert!(
        real < SAME,
        "signing time depends on the message: the classes are {real:.4} of a signature apart \
         (threshold {SAME}). Fixed {a} ns, varying {b} ns at the {PERCENTILE}th percentile."
    );
}
