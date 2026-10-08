//! The coverage instrument for the steward families (kernel/model.md, "Property families"): the
//! lowest seed at which each of the core's rules, each property and each mutation is first
//! exercised, so that a family's seed count is set from what its seeds reach, not by habit.
//!
//! A rule is an entry of the core's `Policy` table: the instrument's table records, per seed,
//! each guard that held and each that refused, each effect that ran and each filter's answers,
//! then does what the shipped entry does. A property is reached at its first instance
//! (`Run::reached`: a declassification checked, a blame, a lease ended). A mutation is reached
//! where a family first catches it.
//!
//!     REDOUBT_MODEL_SEQUENCES=20000 cargo test -p redoubt-model --release --test steward_reach \
//!         -- --ignored --nocapture reach_table
//!     cargo test -p redoubt-model --release --test steward_reach -- --ignored --nocapture catch_table

mod common;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use common::*;
use redoubt_model::check::Failure;
use redoubt_model::mutation::Mutation;
use redoubt_model::policy;
use redoubt_steward::Policy;
use redoubt_steward::audit::Audit;
use redoubt_steward::cx::Cx;
use redoubt_steward::domain::{Domain, Labels};
use redoubt_steward::effect::Refusal;
use redoubt_steward::manifest::Principal;

thread_local! {
    /// What the instrument's table saw during the seed this thread runs.
    static SEEN: RefCell<BTreeSet<&'static str>> = const { RefCell::new(BTreeSet::new()) };
}

fn saw(what: &'static str) { SEEN.with(|s| s.borrow_mut().insert(what)); }

/// One wrapper per entry of `Policy`: it records what it decided and decides as `SHIPPED` does.
macro_rules! instrumented {
    (guards: $($g:ident),*; effects: $($e:ident),*;) => {
        mod wrap {
            use super::*;
            $(
                pub fn $g(cx: &Cx<'_>) -> Result<(), Refusal> {
                    let r = (Policy::SHIPPED.$g)(cx);
                    saw(if r.is_ok() {
                        concat!("rule ", stringify!($g), " holds")
                    } else {
                        concat!("rule ", stringify!($g), " refuses")
                    });
                    r
                }
            )*
            $(
                pub fn $e(cx: &mut Cx<'_>) {
                    saw(concat!("effect ", stringify!($e)));
                    (Policy::SHIPPED.$e)(cx)
                }
            )*
            pub fn audit_visible(labels: &Labels, a: &Audit) -> bool {
                let r = (Policy::SHIPPED.audit_visible)(labels, a);
                saw(if r { "filter audit_visible shows" } else { "filter audit_visible hides" });
                r
            }
            pub fn reaches(p: &Principal, d: &Domain) -> bool {
                let r = (Policy::SHIPPED.reaches)(p, d);
                saw(if r { "filter reaches shows" } else { "filter reaches hides" });
                r
            }
        }

        /// Every name the wrappers can record: each guard both ways, each effect, each filter
        /// both ways.
        const RULES: &[&str] = &[
            $(concat!("rule ", stringify!($g), " holds"), concat!("rule ", stringify!($g), " refuses"),)*
            $(concat!("effect ", stringify!($e)),)*
            "filter audit_visible shows",
            "filter audit_visible hides",
            "filter reaches shows",
            "filter reaches hides",
        ];

        /// `Policy::SHIPPED`, every entry wrapped.
        fn table() -> Policy {
            Policy {
                $($g: wrap::$g,)*
                $($e: wrap::$e,)*
                audit_visible: wrap::audit_visible,
                reaches: wrap::reaches,
            }
        }
    };
}

instrumented! {
    guards: agent_own_set, approval_key, blame_window, caller_unlabelled, context_free, copying, declassifies,
        exact_labels, fair_share, grants_lease, granted, hash_matches, item_fits, lease_bounded,
        login_key, not_locked, owns_labels, pending_cap, pushes, reading, rendered_here,
        sponsor_session;
    effects: audit_agent_started, audit_approved, audit_blamed, audit_copy_failed,
        audit_declassified, audit_denied, audit_lease_ended, audit_locked_out, audit_login,
        audit_push_failed, audit_pushed, audit_start_failed, audit_submitted, carve_crossing,
        carve_lease, carve_session, connect, copy_out, count_blame, create_scope, destroy_budget,
        destroy_crossing, destroy_partial, drop_requests, forget, freeze, grant_lease, launch,
        lock_out, notify, notify_sponsor, open_copy_out, open_read, open_write, pass_failure,
        pass_snapshot, read_item, read_source, refuse, render, reply_agent, reply_login, reply_ok,
        reply_request, route, unroute, write_item;
}

type Reach = fn(u64, Policy) -> Result<BTreeSet<&'static str>, Failure>;

/// The steward families, with the depths the instrument searches: past their counts
/// (properties.rs), so that an item first reached late shows.
const STEWARD: [(&str, Reach, u64); 2] = [
    ("steward_policy", policy::steward_policy_reach, 20_000),
    ("steward_noninterference", policy::steward_noninterference_reach, 10_000),
];

/// Each family's last new item and how many it reaches, as kernel/model.md's table states them
/// ("Property families"), for this generator.
const LAST: [(u64, usize); 2] = [(3470, 118), (405, 120)];

/// Seeds `0..n` of `f`, unmutated, on the instrument's table: the lowest seed reaching each rule
/// and property. Every seed must hold.
fn reach(name: &str, f: Reach, n: u64) -> BTreeMap<&'static str, u64> {
    let next = AtomicU64::new(0);
    let first: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());
    std::thread::scope(|s| {
        for _ in 0..threads() {
            s.spawn(|| {
                let mut mine: BTreeMap<&'static str, u64> = BTreeMap::new();
                loop {
                    let seed = next.fetch_add(1, Ordering::Relaxed);
                    if seed >= n {
                        break;
                    }
                    SEEN.with(|s| s.borrow_mut().clear());
                    let props =
                        f(seed, table()).unwrap_or_else(|e| panic!("{name} seed {seed}: {}", e.message));
                    for what in SEEN.with(|s| s.take()).into_iter().chain(props) {
                        let at = mine.entry(what).or_insert(seed);
                        *at = (*at).min(seed);
                    }
                }
                let mut all = first.lock().unwrap();
                for (what, seed) in mine {
                    let at = all.entry(what).or_insert(seed);
                    *at = (*at).min(seed);
                }
            });
        }
    });
    first.into_inner().unwrap()
}

/// The table on the page is this generator's: each family reaches its last new item at the seed
/// the page states, with as many items. A change to the generator, the core or the checks that
/// moves either fails here, and the counts are set again from `reach_table`.
#[test]
fn the_reach_table_is_reproduced() {
    for ((name, f, _), (last, items)) in STEWARD.into_iter().zip(LAST) {
        let first = reach(name, f, last + 1);
        assert_eq!((first.values().max(), first.len()), (Some(&last), items), "{name}");
    }
}

/// The table for the page: per family, each rule's and property's first seed, the last of them,
/// and what was never reached.
#[test]
#[ignore]
fn reach_table() {
    for (name, f, default) in STEWARD {
        let n = sequences(default);
        let t = std::time::Instant::now();
        let first = reach(name, f, n);
        let mut by_seed: Vec<(u64, &str)> = first.iter().map(|(w, s)| (*s, *w)).collect();
        by_seed.sort();
        for (seed, what) in &by_seed {
            eprintln!("{name:24} {seed:6} {what}");
        }
        let never: Vec<&str> = RULES.iter().copied().filter(|r| !first.contains_key(r)).collect();
        eprintln!("{name}: never reached: {never:?}");
        let last = by_seed.last().copied().unwrap_or_default();
        eprintln!(
            "{name}: {} reached over {n} seeds in {:.0} s; the last at seed {} ({})",
            by_seed.len(),
            t.elapsed().as_secs_f64(),
            last.0,
            last.1
        );
    }
}

/// The lowest seed at which each steward family catches each mutation whose rule the steward
/// families hold (the `Policy` variants and `R2OneCursor`), searching each family to its count.
#[test]
#[ignore]
fn catch_table() {
    quiet_panics();
    let only = std::env::var("REDOUBT_MODEL_MUTATIONS").unwrap_or_default();
    for m in Mutation::ALL {
        if !(m.is_policy() || m == Mutation::R2OneCursor) {
            continue;
        }
        if !only.is_empty() && !only.split(',').any(|w| format!("{m:?}").contains(w)) {
            continue;
        }
        let mut line = format!("{:32}", format!("{m:?}"));
        for (i, (name, _, default)) in STEWARD.iter().enumerate() {
            let (_, f, _) = FAMILIES[3 + i];
            let t = std::time::Instant::now();
            let caught = run(name, f, sequences(*default), Some(m));
            line += &match caught {
                Some(c) => format!(" {name} {:>6} ({:.0} s)", c.seed, t.elapsed().as_secs_f64()),
                None => format!(" {name}      - ({:.0} s)", t.elapsed().as_secs_f64()),
            };
        }
        eprintln!("{line}");
    }
}
