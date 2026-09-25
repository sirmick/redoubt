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
        let mut caught = common::contracts::ipc_contracts(Some(m))
            .err()
            .map(|message| redoubt_model::check::Failure {
                family: "ipc_contracts",
                seed: 0,
                message,
                ops: vec![],
            })
            .or_else(|| {
                common::contracts::sched_contracts(Some(m)).err().map(|message| {
                    redoubt_model::check::Failure { family: "sched_contracts", seed: 0, message, ops: vec![] }
                })
            });
        // Try the rule's pressure family first, retaining every family and unchanged seed caps.
        let preferred = match m.rule() {
            "R12" => "scheduler_fairness",
            // These breaks expose confidential work through shared state or audit reads.
            // Try their paired-world oracle before spending full caps on unrelated families.
            "policy"
                if matches!(
                    m,
                    Mutation::PolicyCapPerAccount
                        | Mutation::PolicySequentialIds
                        | Mutation::PolicyAuditUnfiltered
                ) =>
            {
                "steward_noninterference"
            }
            "policy" => "steward_policy",
            _ if matches!(
                m,
                Mutation::R4aOpenCallsPerThread
                    | Mutation::R4aFullTakesNothing
                    | Mutation::OpenCallsUnlimited
            ) =>
            {
                "flood"
            }
            _ => "kernel_sequence",
        };
        let mut families = FAMILIES;
        families.sort_by_key(|(name, _, _)| *name != preferred);
        for (name, f, divisor) in families {
            if caught.is_some() {
                break;
            }
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
