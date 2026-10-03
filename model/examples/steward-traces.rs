//! `cargo run -p redoubt-model --example steward-traces -- DIR` writes, into DIR, the events the
//! model's steward families drive into the core at fixed seeds, one trace per run, for the Elixir
//! reference's differential run (servers/steward.md, "Two embedders and a reference"):
//! `model-policy-SEED.trace` (P1-P9, P11-P14), and `model-noninterference-SEED-with.trace` and
//! `-without.trace` (P10's two runs).

use redoubt_model::policy::{manifest, steward_noninterference_events, steward_policy_events};
use redoubt_steward::Event;
use redoubt_steward_trace::record::trace;

/// The seeds: fixed, so every run of the bench checks the same traces.
const POLICY: [u64; 4] = [1, 2, 3, 4];
const NONINTERFERENCE: [u64; 1] = [1];

fn main() {
    let dir = std::env::args().nth(1).expect("usage: steward-traces DIR");
    let m = manifest();
    let write = |name: String, comment: String, events: &[Event]| {
        // A trace with no events checks nothing.
        assert!(!events.is_empty(), "{name}: the run drove no events");
        let path = std::path::Path::new(&dir).join(name);
        std::fs::write(&path, trace(&comment, &m, events))
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    };
    for seed in POLICY {
        let events = steward_policy_events(seed).unwrap_or_else(|f| panic!("{}", f.message));
        write(format!("model-policy-{seed}.trace"), format!("steward_policy, seed {seed}"), &events);
    }
    for seed in NONINTERFERENCE {
        let [with, without] =
            steward_noninterference_events(seed).unwrap_or_else(|f| panic!("{}", f.message));
        let comment = |w: &str| format!("steward_noninterference, seed {seed}, the run {w} the vault's work");
        write(format!("model-noninterference-{seed}-with.trace"), comment("with"), &with);
        write(format!("model-noninterference-{seed}-without.trace"), comment("without"), &without);
    }
}
