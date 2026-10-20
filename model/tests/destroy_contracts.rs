//! `budget_destroy` among live siblings (kernel/budgets.md, R10): the model's own check that a
//! destroy ends exactly the subtree it names, in the orders a sponsor's leases end by exit.

mod common;

#[test]
fn a_siblings_destroy_spares_its_neighbours() {
    common::contracts::sibling_destroy_spares_neighbours(None).unwrap();
}
