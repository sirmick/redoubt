//! The property-test runner shared by the test files: runs a family's seeds on every core,
//! turns a panic into a failure (I14), and reports a kernel failure as a shrunk trace.

#![allow(dead_code)]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use redoubt_model::check::{self, Failure};
use redoubt_model::kernel::Boot;
use redoubt_model::mutation::Mutation;
use redoubt_model::{policy, trace};

pub type Family = fn(u64, Option<Mutation>) -> Result<(), Failure>;

/// Every property family, in the order the mutation check tries them.
pub const FAMILIES: [(&str, Family); 5] = [
    ("kernel_sequence", check::kernel_sequence),
    ("budget_lifecycle", check::budget_lifecycle),
    ("scheduler_fairness", check::scheduler_fairness),
    ("steward_policy", policy::steward_policy),
    ("steward_noninterference", policy::steward_noninterference),
];

/// Sequences per family: `REDOUBT_MODEL_SEQUENCES`, or `default`.
pub fn sequences(default: u64) -> u64 {
    std::env::var("REDOUBT_MODEL_SEQUENCES")
        .ok()
        .and_then(|s| s.replace('_', "").parse().ok())
        .unwrap_or(default)
}

/// Run seeds `0..n` of `f` on all cores. Returns the failure with the lowest seed found (the
/// search stops early once one is found), counting a panic as a failure of I14.
pub fn run(name: &'static str, f: Family, n: u64, mutation: Option<Mutation>) -> Option<Failure> {
    let threads = std::thread::available_parallelism().map_or(4, |x| x.get()) as u64;
    let next = AtomicU64::new(0);
    let stop = AtomicBool::new(false);
    let found: Mutex<Option<Failure>> = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    // Seeds in blocks of 64, to keep the counter cold.
                    let first = next.fetch_add(64, Ordering::Relaxed);
                    if first >= n || stop.load(Ordering::Relaxed) {
                        return;
                    }
                    for seed in first..(first + 64).min(n) {
                        let r = catch_unwind(AssertUnwindSafe(|| f(seed, mutation)));
                        let failure = match r {
                            Ok(Ok(())) => continue,
                            Ok(Err(e)) => e,
                            Err(p) => {
                                let what = p
                                    .downcast_ref::<String>()
                                    .cloned()
                                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                                    .unwrap_or_default();
                                Failure {
                                    family: name,
                                    seed,
                                    message: format!("I14: the model panicked: {what}"),
                                    ops: vec![],
                                }
                            }
                        };
                        let mut g = found.lock().unwrap();
                        if g.as_ref().is_none_or(|x| failure.seed < x.seed) {
                            *g = Some(failure);
                        }
                        stop.store(true, Ordering::Relaxed);
                        return;
                    }
                }
            });
        }
    });
    found.into_inner().unwrap()
}

/// A failure, explained: for a kernel sequence, shrunk and printed as a trace to replay.
pub fn explain(f: &Failure, mutation: Option<Mutation>) -> String {
    let mut s = format!("{} seed {}: {}", f.family, f.seed, f.message);
    if !f.ops.is_empty() {
        let boot = Boot::default();
        let small = catch_unwind(AssertUnwindSafe(|| check::shrink(&boot, &f.ops, mutation)))
            .unwrap_or(f.ops.clone());
        let why = check::replay(&boot, &small, mutation).err().unwrap_or_default();
        s += &format!(
            "\nshrunk to {} ops ({why}); trace:\n{}",
            small.len(),
            trace::record(&boot, &small, mutation).unwrap_or_default()
        );
    }
    s
}

/// Silence the default panic message: panics are counted as I14 failures and reported.
pub fn quiet_panics() {
    std::panic::set_hook(Box::new(|_| {}));
}
