//! The tests are not vacuous: each deliberate rule break (mutation.rs) must make some property
//! fail, and every rule R1-R12 must have at least one such break.
//!
//!     cargo test -p redoubt-model --release --test mutations -- --nocapture
//!
//! prints, for each mutation, the first property that caught it and the seed.

mod common;

use common::*;
use redoubt_model::mutation::Mutation;

/// Seeds tried per family before a mutation counts as not caught.
const CAP: u64 = 20_000;

#[test]
fn every_rule_has_a_mutation() {
    for r in 1..=12 {
        let rule = format!("R{r}");
        assert!(Mutation::ALL.iter().any(|m| m.rule() == rule), "no mutation breaks {rule}");
    }
}

#[test]
fn mutations_are_caught() {
    quiet_panics();
    let mut missed = Vec::new();
    // `REDOUBT_MODEL_MUTATIONS=R10,Policy` checks only mutations whose name contains one of those.
    let only = std::env::var("REDOUBT_MODEL_MUTATIONS").unwrap_or_default();
    let wanted = |m: &Mutation| only.is_empty() || only.split(',').any(|s| format!("{m:?}").contains(s));
    for m in Mutation::ALL.into_iter().filter(wanted) {
        let mut caught = None;
        for (name, f, divisor) in FAMILIES {
            if let Some(fail) = run(name, f, sequences(CAP).min(CAP).div_ceil(divisor), Some(m)) {
                caught = Some(fail);
                break;
            }
        }
        match caught {
            Some(f) => eprintln!(
                "{:6} {:32} caught by {} seed {}: {}",
                m.rule(),
                format!("{m:?}"),
                f.family,
                f.seed,
                f.message
            ),
            None => {
                eprintln!("{:6} {:32} NOT CAUGHT", m.rule(), format!("{m:?}"));
                missed.push(m);
            }
        }
    }
    assert!(missed.is_empty(), "mutations no property caught: {missed:?}");
}
