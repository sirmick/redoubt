//! A budget's labels are a set (docs/kernel/budgets.md, "Labels on budgets"): the kernel sorts the
//! spec's labels and drops repeats, and a user-class caller's child must name exactly its parent's
//! set. The fake keeps sets as the kernel does, so a parent made from labels in any order carves a
//! child that names the same set in any order, and refuses one with fewer or more.

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{BudgetSpec, Error, FOREVER, Labels};
use redoubt_rt::handle::Budget;

#[test]
fn a_child_names_its_parents_set_in_any_order() {
    let f = fake();
    let pid = f.process(1001, &[9, 5, 9]);
    let parent = Budget::from_handle(f.budget(pid));
    let carve = |labels: &[u64]| {
        let spec = BudgetSpec {
            pages: 1,
            processes: 0,
            weight: 0,
            labels: Labels::from_slice(labels).unwrap(),
            account: 0,
            deadline: FOREVER,
        };
        f.as_process(pid, || parent.create_child(&spec).map(drop))
    };
    for same in [&[5, 9][..], &[9, 5], &[9, 5, 5]] {
        assert_eq!(carve(same), Ok(()), "{same:?}");
    }
    assert_eq!(carve(&[9]), Err(Error::LabelDenied));
    assert_eq!(carve(&[]), Err(Error::LabelDenied));
    assert_eq!(carve(&[11, 9, 5]), Err(Error::ClassDenied));
}
