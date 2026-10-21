//! `endpoint_create` naming a budget (kernel/objects.md): the model's own checks, independent of
//! the kernel boot bench (`bench:budget-syscall-attack` covers the kernel). `mutations_are_caught`
//! runs the same scenario under every mutation, through `ipc_contracts`.

mod common;

#[test]
fn an_endpoint_named_into_a_budget_below_is_owned_by_it_and_dies_with_it() {
    common::contracts::endpoint_create_owned(None).unwrap();
}
