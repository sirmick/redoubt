//! The tests are not vacuous: each deliberate rule break (mutation.rs) must make some property
//! fail, and every kernel rule the model holds must have at least one such break.
//!
//!     cargo test -p redoubt-model --release --test mutations -- --nocapture
//!
//! prints, for each mutation, the first property that caught it and the seed.

mod common;

use common::*;
use redoubt_model::mutation::Mutation;

/// Seeds tried per family before a mutation counts as not caught.
const CAP: u64 = 20_000;

/// Seeds tried in each steward family: a mutation these catch only past it is caught too late,
/// and fails as not caught. The random search catches every one by seed 488 but
/// `PolicyDeclassifyUnfit` (`steward_policy`'s seed 1,418), which its scenario catches first
/// (`steward_scenario`), as it does `R2OneCursor`, `PolicyAgentOtherSet` and
/// `PolicyEndLeaseAdmitted`, which `steward_policy` misses within the cap and
/// `steward_noninterference` catches at seed 126, too slowly for the bench's deadline. With
/// `TESTBENCH_LATE` set, the bench's word that the mutation is known to be late, the steward
/// families get `CAP` too, and one caught within the caps after all fails, so that its entry is
/// removed.
const STEWARD_CAP: u64 = 500;

/// Every kernel rule the model holds, and I16: all of R1 to R24 but the six outside the model
/// (kernel/model.md, "Mutations"), with R4a and R4b beside R4.
const MODELLED: [&str; 21] = [
    "R1", "R2", "R3", "R4", "R4a", "R4b", "R5", "R6", "R7", "R8", "R9", "R10", "R11", "R12", "R13", "R14",
    "R18", "R20", "R21", "R22", "I16",
];

#[test]
fn every_rule_has_a_mutation() {
    for rule in MODELLED {
        assert!(Mutation::ALL.iter().any(|m| m.rule() == rule), "no mutation breaks {rule}");
    }
}

/// A scenario holds on the specified model and catches its break, with the property it names.
fn scenario(f: impl Fn(Option<Mutation>) -> Result<(), String>, m: Mutation, property: &str) {
    if let Err(e) = f(None) {
        panic!("the specified model fails {m:?}'s scenario: {e}");
    }
    match f(Some(m)) {
        Err(e) => assert!(e.starts_with(property), "{m:?} caught by {e}, not {property}"),
        Ok(()) => panic!("{m:?}'s scenario does not catch it"),
    }
}

#[test]
fn declassify_unfit_scenario() {
    for item in common::contracts::UNFIT {
        let f = |m| common::contracts::declassify_unfit_item(m, item);
        scenario(f, Mutation::PolicyDeclassifyUnfit, "P6:");
    }
}

#[test]
fn declassify_live_scenario() {
    scenario(common::contracts::declassify_live, Mutation::PolicyDeclassifyLive, "P6:")
}

#[test]
fn one_cursor_scenario() { scenario(common::contracts::one_cursor, Mutation::R2OneCursor, "P10:") }

#[test]
fn end_lease_admitted_scenario() {
    scenario(common::contracts::end_lease_admitted, Mutation::PolicyEndLeaseAdmitted, "P13:")
}

#[test]
fn agent_other_set_scenario() {
    scenario(common::contracts::agent_other_set, Mutation::PolicyAgentOtherSet, "P10:")
}

#[test]
fn mutations_are_caught() {
    quiet_panics();
    let mut missed = Vec::new();
    // `REDOUBT_MODEL_MUTATIONS=R10,Policy` checks only mutations whose name contains one of those;
    // a word that is a whole name takes that mutation alone (the bench's one job per mutation).
    let only = std::env::var("REDOUBT_MODEL_MUTATIONS").unwrap_or_default();
    let names: Vec<String> = Mutation::ALL.iter().map(|m| format!("{m:?}")).collect();
    let wanted = |m: &Mutation| {
        let name = format!("{m:?}");
        only.is_empty()
            || only
                .split(',')
                .any(|s| if names.iter().any(|n| n == s) { name == s } else { name.contains(s) })
    };
    let late = std::env::var_os("TESTBENCH_LATE").is_some();
    // Known late but caught within the caps: the bench's entry for it has outlived its need.
    let mut early = Vec::new();
    let mut checked = 0;
    for m in Mutation::ALL.into_iter().filter(wanted) {
        checked += 1;
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
            })
            .or_else(|| {
                // The steward's directed scenario for this break, before the random search.
                common::contracts::steward_scenario(m)?.err().map(|message| redoubt_model::check::Failure {
                    family: "steward_scenario",
                    seed: 0,
                    message,
                    ops: vec![],
                })
            });
        // Try the rule's pressure family first, retaining every family and unchanged seed caps.
        let preferred = match m.rule() {
            "R12" => "scheduler_fairness",
            // These breaks expose confidential work through shared state, audit reads, another
            // domain's records, or the order a server takes unlabelled calls in. Try their
            // paired-world oracle before spending full caps on unrelated families.
            _ if matches!(
                m,
                Mutation::PolicySequentialIds
                    | Mutation::PolicyAuditUnfiltered
                    | Mutation::PolicyAgentOtherSet
                    | Mutation::R2OneCursor
            ) =>
            {
                "steward_noninterference"
            }
            _ if m.is_policy() => "steward_policy",
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
            let cap = if name.starts_with("steward_") && !late { STEWARD_CAP } else { CAP };
            if let Some(fail) = run(name, f, sequences(CAP).min(cap).div_ceil(divisor), Some(m)) {
                caught = Some(fail);
                break;
            }
        }
        match caught {
            Some(f) => {
                let past = f.family.starts_with("steward_") && f.seed >= STEWARD_CAP;
                eprintln!(
                    "{:6} {:32} caught by {} seed {}{}: {}",
                    m.rule(),
                    format!("{m:?}"),
                    f.family,
                    f.seed,
                    if past { ", late, known" } else { "" },
                    f.message
                );
                if late && !past {
                    early.push(m);
                }
            }
            None => {
                eprintln!("{:6} {:32} NOT CAUGHT within the caps", m.rule(), format!("{m:?}"));
                missed.push(m);
            }
        }
    }
    // A word naming no mutation would make a run that checks nothing pass.
    assert!(only.is_empty() || checked > 0, "no mutation named by {only:?}");
    assert!(missed.is_empty(), "mutations no property caught within the caps: {missed:?}");
    assert!(early.is_empty(), "caught within the caps, so no longer late: remove the entry for {early:?}");
}
