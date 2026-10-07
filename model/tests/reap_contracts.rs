//! `budget_reap` (kernel/budgets.md, R10): the model's own checks, independent of the kernel boot
//! bench (`bench:budget-reap` covers the kernel). `mutations_are_caught` runs the same scenario
//! under every mutation, through `ipc_contracts`.

mod common;

#[test]
fn a_reap_destroys_one_child_and_keeps_the_budget() {
    common::contracts::reap_empties_and_keeps(None).unwrap();
}
