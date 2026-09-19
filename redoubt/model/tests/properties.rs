//! The property tests: every family over many random sequences, on the specified model (no
//! mutation). Each must hold for every seed.
//!
//!     cargo test -p redoubt-model --release --test properties
//!     REDOUBT_MODEL_SEQUENCES=1000000 cargo test -p redoubt-model --release --test properties
//!
//! The default is 20,000 sequences per family (a few seconds on a many-core machine); the
//! acceptance run for WP-M0 is 10^6 per family, also reachable as the ignored test `million`.

mod common;

use common::*;

fn family(i: usize, default: u64) {
    let (name, f) = FAMILIES[i];
    let n = sequences(default);
    if let Some(fail) = run(name, f, n, None) {
        panic!("{}", explain(&fail, None));
    }
    eprintln!("{name}: {n} sequences, all properties hold");
}

#[test]
fn kernel_sequences() { family(0, 20_000) }

#[test]
fn budget_lifecycles() { family(1, 20_000) }

#[test]
fn scheduler_fairness() { family(2, 20_000) }

#[test]
fn steward_policy() { family(3, 20_000) }

#[test]
fn steward_noninterference() { family(4, 10_000) }

/// The acceptance run: 10^6 sequences of every family.
#[test]
#[ignore]
fn million() {
    for (i, (name, _)) in FAMILIES.iter().enumerate() {
        let t = std::time::Instant::now();
        family(i, 1_000_000);
        eprintln!("{name}: {:.1} s", t.elapsed().as_secs_f64());
    }
}
