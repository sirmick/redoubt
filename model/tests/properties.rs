//! The property tests: every family over many random sequences, on the specified model (no
//! mutation). Each must hold for every seed.
//!
//!     cargo test -p redoubt-model --release --test properties
//!     REDOUBT_MODEL_SEQUENCES=1000000 cargo test -p redoubt-model --release --test properties
//!
//! Defaults: 20,000 sequences each for kernel, budget lifecycle, scheduler and steward policy;
//! 10,000 for steward noninterference; 20 flood sequences (90,020 total).
//! Without a sequence-count environment override, the ignored `million` acceptance test runs
//! 1,000,000 sequences in each of the five non-flood families and 1,000 flood sequences
//! (5,001,000 total).

mod common;

use common::*;

fn family(i: usize, default: u64) {
    let (name, f, divisor) = FAMILIES[i];
    let n = sequences(default).div_ceil(divisor);
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

#[test]
fn flood() { family(5, 20_000) }

/// Acceptance without an environment override: five families of 1,000,000 sequences plus
/// 1,000 flood sequences, for 5,001,000 total.
#[test]
#[ignore]
fn million() {
    for (i, (name, _, _)) in FAMILIES.iter().enumerate() {
        let t = std::time::Instant::now();
        family(i, 1_000_000);
        eprintln!("{name}: {:.1} s", t.elapsed().as_secs_f64());
    }
}
