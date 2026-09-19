//! The timing check for the signing path (BUILD-PLAN.md, WP-S1: "the signing path is
//! constant-time under the bench's timing check"; CONTAINMENT.md, covert and timing channels;
//! TENETS.md: assume the attacker has a perfect clock).
//!
//! **What it measures.** How long one signature takes, as a function of the two secrets the
//! signing path touches: the key, and the nonce, which Ed25519 derives from the message. It
//! times eight different keys over the same message, and eight different messages under the
//! same key. For each of the eight it takes the **minimum** batch mean over several repeats —
//! the minimum, because noise can only push a measurement up, so the smallest one seen is the
//! closest to the work actually done — and then compares the largest of those minima with the
//! smallest. If the work depended on the secret, the eight would not agree.
//!
//! **What it proves, and what it does not.** It proves that this build of this Ed25519
//! implementation does not take grossly different amounts of time for different secrets: it
//! would catch a double-and-add loop that skipped zero bits, a windowed multiply that indexed a
//! table by secret bits and hit different cache lines, or a hand-rolled decoder branching on
//! key bytes. It is not a proof of constant time, and it says nothing about
//! microarchitectural channels, which TENETS.md puts out of scope for the software. The claim
//! that the path is constant time rests on reading the code (`keys::Key::sign` says what was
//! read); this test is what would notice if that stopped being true.
//!
//! **Why it is not flaky.** The verdict is a ratio of minima, which noise inflates only
//! upwards and only for the batch it lands in. And the test carries its own control
//! (TENETS.md 6, "the harness can fail"): a deliberately variable-time signer, whose work
//! depends on the key's bits, measured by the same code in the same run. The control has to be
//! flagged, or the test fails whatever the real signer did — so a machine too noisy to tell the
//! two apart reports that, rather than passing by accident.

use std::time::{Duration, Instant};

use ed25519_compact::sha512;
use redoubt_keyd::keys::Keys;

/// Signatures in one batch.
const BATCH: usize = 25;
/// How often each secret is measured; the smallest result counts.
const REPEATS: usize = 7;
/// The secrets compared against each other.
const SECRETS: usize = 8;

/// How far apart the fastest and the slowest secret may be. A constant-time implementation
/// lands within a few per cent; the control below is a hundred times outside it, so nothing
/// sensible sits near this line.
const SAME: f64 = 1.30;
/// How far apart the control must be, for this run to count as able to tell the difference.
const DIFFERENT: f64 = 2.0;

/// Eight seeds: one set bit, all bits set, and six spread between, so a signer whose work
/// followed the key's bit pattern could not hide. (Not all zero: that is the one seed
/// `ed25519-compact` panics on, which `Keys` refuses.)
fn seeds() -> Vec<[u8; 32]> {
    let mut lowest = [0x00u8; 32];
    lowest[0] = 0x01;
    let mut seeds = vec![lowest, [0xffu8; 32]];
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..SECRETS - 2 {
        let mut seed = [0u8; 32];
        for byte in seed.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *byte = x as u8;
        }
        seeds.push(seed);
    }
    seeds
}

fn hex(seed: &[u8; 32]) -> String { seed.iter().map(|b| format!("{b:02x}")).collect() }

fn keys(seeds: &[[u8; 32]]) -> Keys {
    let args: Vec<String> =
        seeds.iter().enumerate().map(|(i, seed)| format!("k{i},audit,{}", hex(seed))).collect();
    Keys::from_args(args.iter().map(String::as_str)).unwrap()
}

/// Eight 64-byte messages, so the nonce (which Ed25519 derives from the message, and which the
/// scalar multiplication then uses) differs between them.
fn messages() -> Vec<[u8; 64]> {
    (0..SECRETS)
        .map(|i| {
            let mut m = [0u8; 64];
            for (j, byte) in m.iter_mut().enumerate() {
                *byte = (i * 37 + j * 11) as u8;
            }
            m
        })
        .collect()
}

/// The smallest batch mean over [`REPEATS`] repeats, for each of `n` cases. The repeats are
/// interleaved, so a slow patch of the machine falls on every case rather than on one.
fn minima(n: usize, mut work: impl FnMut(usize)) -> Vec<Duration> {
    // Warm up: the first calls pay for page faults and branch predictor training.
    for case in 0..n {
        for _ in 0..BATCH {
            work(case);
        }
    }
    let mut best = vec![Duration::MAX; n];
    for _ in 0..REPEATS {
        for (case, best) in best.iter_mut().enumerate() {
            let start = Instant::now();
            for _ in 0..BATCH {
                work(case);
            }
            *best = (*best).min(start.elapsed() / BATCH as u32);
        }
    }
    best
}

/// The largest of the minima over the smallest: 1.0 when every case does the same work.
fn spread(minima: &[Duration]) -> f64 {
    let slowest = minima.iter().max().unwrap().as_secs_f64();
    let fastest = minima.iter().min().unwrap().as_secs_f64();
    assert!(fastest > 0.0, "the clock is too coarse to measure a signature on this machine");
    slowest / fastest
}

/// A signer whose work depends on its key, which is what this test exists to catch. It is the
/// control: the same measurement must flag it, in the same run, or the run proves nothing.
fn variable_time_sign(seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    let mut state = sha512::Hash::hash(seed);
    for byte in seed {
        for bit in 0..8 {
            if (byte >> bit) & 1 == 1 {
                state = sha512::Hash::hash(state);
            }
        }
    }
    let mut hasher = sha512::Hash::new();
    hasher.update(state);
    hasher.update(message);
    hasher.finalize()
}

#[test]
fn signing_takes_the_same_time_whatever_the_key() {
    let seeds = seeds();
    let keys = keys(&seeds);
    let message = [0x5au8; 64];

    // The control first, so a run that cannot tell the two apart says so before anything else.
    let control = minima(SECRETS, |case| {
        std::hint::black_box(variable_time_sign(&seeds[case], &message));
    });
    let control_spread = spread(&control);
    assert!(
        control_spread > DIFFERENT,
        "the control, whose work follows its key's bits, was not flagged (spread {control_spread:.3}): \
         this machine is too noisy for the measurement to mean anything"
    );

    let real = minima(SECRETS, |case| {
        std::hint::black_box(keys.get(case).unwrap().sign(&message));
    });
    let real_spread = spread(&real);
    assert!(
        real_spread < SAME,
        "signing time depends on the key (spread {real_spread:.3}, control {control_spread:.3}); \
         minima {real:?}"
    );
}

#[test]
fn signing_takes_the_same_time_whatever_the_nonce() {
    // Ed25519's nonce comes from the key's prefix and the message, and the scalar
    // multiplication uses it. Different messages of the same length therefore exercise
    // different secret scalars, which is the value the multiplication must not leak.
    let seeds = seeds();
    let keys = keys(&seeds[..1]);
    let key = keys.get(0).unwrap();
    let messages = messages();
    let real = minima(SECRETS, |case| {
        std::hint::black_box(key.sign(&messages[case]));
    });
    let real_spread = spread(&real);
    assert!(real_spread < SAME, "signing time depends on the message (spread {real_spread:.3}); {real:?}");
}
