//! The steward's start (servers/steward.md, "Principals" and "Fixed sub-budgets per label set"):
//! its manifest lines, the core's boot, and boot's carve as kernel calls, against a kernel that
//! records them.

use redoubt_rt::abi::{BudgetSpec, Error, FOREVER};
use redoubt_steward_server::{Kernel, StartError, start};

const USERS: usize = 0;
const SERVERS: &str = "servers 5";
const SIZES: &str = "sizes session=10,2,5 agent=10,2,5 sub_agent=5,1,2 crossing=2,1,1 cost=1";
const ALICE: &str =
    "principal \"alice\" account=1001 login=[11] approval=[21] owned=[7] sets=[[],[7]] top=1000,30,300";
const BOB: &str = "principal \"bob\" account=1002 login=[12] approval=[22] owned=[] sets=[[]] top=500,10,100";

/// Budget `i` is the `i`th made; `USERS` is the one the steward is handed.
#[derive(Default)]
struct Recorder {
    made: Vec<(usize, BudgetSpec)>,
    fail_at: Option<usize>,
}

impl Kernel for Recorder {
    type Budget = usize;

    fn create(&mut self, parent: usize, spec: &BudgetSpec) -> Result<usize, Error> {
        if self.fail_at == Some(self.made.len()) {
            return Err(Error::OutOfMemory);
        }
        self.made.push((parent, *spec));
        Ok(self.made.len())
    }
}

fn shape(made: &[(usize, BudgetSpec)]) -> Vec<(usize, u64, u32, u32, Vec<u64>, u64)> {
    made.iter()
        .map(|(p, s)| {
            assert_eq!(s.deadline, FOREVER);
            (*p, s.pages, s.processes, s.weight, s.labels.as_slice().to_vec(), s.account)
        })
        .collect()
}

#[test]
fn each_principal_gets_a_top_budget_under_users_and_a_sub_budget_per_label_set() {
    let mut k = Recorder::default();
    let s = start(&[ALICE, BOB, "keyd [31]", SERVERS, SIZES], USERS, &mut k).unwrap();
    // The account on the top budget only, which the kernel stamps on all below it; each
    // sub-budget an equal share of the top, less its own object's page.
    assert_eq!(
        shape(&k.made),
        [
            (USERS, 1000, 30, 300, vec![], 1001),
            (1, 499, 15, 150, vec![], 0),
            (1, 499, 15, 150, vec![7], 0),
            (USERS, 500, 10, 100, vec![], 1002),
            (4, 499, 10, 100, vec![], 0),
        ]
    );
    let names: Vec<(&str, u64, usize, usize)> =
        s.carved.iter().map(|c| (c.name.as_str(), c.account, c.top, c.subs.len())).collect();
    assert_eq!(names, [("alice", 1001, 1, 2), ("bob", 1002, 4, 1)]);
    let alice_vault = &s.carved[0].subs[1].0;
    assert_eq!(s.sub(alice_vault), Some(3));
}

#[test]
fn a_malformed_line_is_a_start_failure_before_any_carve() {
    let mut k = Recorder::default();
    let e = start(&[ALICE, "servers 4 5", SIZES], USERS, &mut k).err().unwrap();
    assert!(matches!(&e, StartError::Lines(why) if why.starts_with("line 2: ")), "{e:?}");
    assert!(k.made.is_empty());
}

#[test]
fn a_servers_count_other_than_the_binding_table_s_is_a_start_failure() {
    for n in [0, 4, 6] {
        let mut k = Recorder::default();
        let servers = format!("servers {n}");
        let e = start(&[ALICE, &servers, SIZES], USERS, &mut k).err().unwrap();
        assert_eq!(e, StartError::Slots(n));
        assert!(k.made.is_empty());
    }
    let mut k = Recorder::default();
    assert_eq!(start(&[ALICE, SIZES], USERS, &mut k).err().unwrap(), StartError::Slots(0));
}

#[test]
fn a_manifest_the_core_refuses_is_a_start_failure_before_any_carve() {
    let mut k = Recorder::default();
    // Bob's login key is alice's approval key: one key in two roles.
    let bob = BOB.replace("login=[12]", "login=[21]");
    let e = start(&[ALICE, &bob, SERVERS, SIZES], USERS, &mut k).err().unwrap();
    assert_eq!(e, StartError::Manifest);
    assert!(k.made.is_empty());
}

#[test]
fn limits_the_kernel_cannot_take_are_a_start_failure() {
    let mut k = Recorder::default();
    let alice = ALICE.replace("top=1000,30,300", "top=1000,30,4294967296");
    let e = start(&[&alice, SERVERS, SIZES], USERS, &mut k).err().unwrap();
    assert_eq!(e, StartError::Limits { account: 1001 });
}

#[test]
fn a_refused_carve_is_a_start_failure_naming_the_budget() {
    for (at, labels) in [(0, vec![]), (2, vec![7])] {
        let mut k = Recorder { fail_at: Some(at), ..Recorder::default() };
        let e = start(&[ALICE, BOB, SERVERS, SIZES], USERS, &mut k).err().unwrap();
        assert_eq!(e, StartError::Carve { account: 1001, labels, error: Error::OutOfMemory });
    }
}
