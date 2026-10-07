# MODEL1 handoff (accepted; merging at head 41c146942 on main be8f7ec0b)

## State
- Branch wp-MODEL1, worktree /home/mcloonan/redoubt/.worktrees/MODEL1. Three commits: 410a43ca8 batched tick (+ equality test, guard == 5), 61061ff48 steward scenarios (late entry and late_cores gone from tests/model-mutations.toml), 41c146942 counts 5,000/7,000 + coverage instrument + page. Nothing pushed by me. Report: .wash/local/MODEL1-report.md ("Final" at the end).
- Scratch only, never committed: target/prof/ (gdb sampler sample.sh/agg.py, logs, copied binaries). Safe to delete.

## Procedures that should outlive this package
### Re-measuring the steward reach table (when the generator, core or checks change and the_reach_table_is_reproduced fails)
1. Build: `q run --cores 4 -- cargo test -p redoubt-model --release --no-run`.
2. Run: `MODEL_THREADS=4 q run --cores 4 -- <target>/release/deps/steward_reach-<hash> --ignored --nocapture --exact reach_table` (searches steward_policy to 20,000 and steward_noninterference to 10,000 seeds: ~3 min on 4 cores at 0.011/0.025 s a seed). REDOUBT_MODEL_SEQUENCES overrides both depths.
3. Output: per item (rule <guard> holds/refuses, effect <name>, filter ..., P<n> <instance>) its first seed; "never reached" list; "N reached ... the last at seed S".
4. Update LAST in model/tests/steward_reach.rs ((655,113),(1295,116) now), the counts in model/tests/properties.rs (family(3,_), family(4,_)), and docs/kernel/model.md "Property families" (table + counts paragraph) and "Residual risks" (never-reached items).
### Re-measuring the catch floor
`REDOUBT_MODEL_MUTATIONS=<words> MODEL_THREADS=4 q run --cores 4 -- <steward_reach bin> --ignored --nocapture --exact catch_table` prints, per Policy/R2OneCursor mutation, the lowest catching seed in each steward family (searches each to its depth; uncaught = '-'). Current: PolicyDeclassifyUnfit 4,709 / 6,333; R2OneCursor - / 345; PolicyAgentOtherSet - / 96. Count rule: 4x last reach rounded up to a thousand, never below a random catch in that family.
### Profiling a model seed
perf is refused (paranoid 4), no valgrind; gdb as parent works (ptrace_scope 1): build with CARGO_PROFILE_RELEASE_DEBUG=1 in a separate CARGO_TARGET_DIR, run the test binary under `gdb -batch` with a command file of `thread apply all bt 60`/`continue` pairs while a loop sends SIGINT every 100 ms (pkill -INT -x <15-char comm>). Script: target/prof/pmp/sample.sh + agg.py in the MODEL1 worktree.

## Traps
- Scenarios run only for their own mutation (contracts::steward_scenario); running them for all lets a steward boot panic pre-empt kernel families (LabelsAddedByParentClass).
- The tick batch condition requires next_event > now and Scheduler::steady(); the equality test's guard asserts exactly 5 queued budgets in kinds 4-6.
- The steward case runs in dev (core unoptimised): ~0.04 s/seed noninterference there vs 0.025 in release.
- Open, not done: neither steward family reaches a failed crossing (audit_copy_failed, audit_push_failed, pass_failure); steward_policy never calls audit_visible. A generator change would be its own package.
