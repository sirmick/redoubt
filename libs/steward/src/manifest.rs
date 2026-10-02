//! What the boot manifest fixes and nothing changes (servers/steward.md, "Principals" and
//! "Fixed sub-budgets per label set"): the principals, their keys, owned labels and label sets,
//! and the keys `keyd` holds.

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use crate::domain::{Domain, Labels};

/// A budget's limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub pages: u64,
    pub processes: u64,
    pub weight: u64,
}

/// One principal as the manifest gives it. Keys are opaque ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrincipalSpec {
    pub name: String,
    pub account: u64,
    pub login_keys: Vec<u64>,
    pub approval_keys: Vec<u64>,
    /// The labels it owns.
    pub owned: Vec<u64>,
    /// The label sets it works under: one domain each. A set need not be owned (a project's
    /// label, say), so ownership is checked on its own (`owns_labels`).
    pub label_sets: Vec<Vec<u64>>,
    /// Its top budget, which boot splits into one fixed sub-budget per label set.
    pub top: Limits,
}

/// The sizes the steward carves, set by the system bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sizes {
    pub session: Limits,
    pub agent: Limits,
    /// A sub-agent, carved inside its agent's budget.
    pub sub_agent: Limits,
    /// A reader or writer budget.
    pub crossing: Limits,
    /// The pages a budget object itself costs, taken from each sub-budget's share.
    pub budget_cost: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub principals: Vec<PrincipalSpec>,
    /// Keys `keyd` holds.
    pub keyd_keys: Vec<u64>,
    /// The shared servers a session's namespace holds a fresh connection to.
    pub servers: u16,
    pub sizes: Sizes,
}

/// A principal, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal {
    pub name: String,
    pub account: NonZeroU64,
    pub login_keys: Vec<u64>,
    pub approval_keys: Vec<u64>,
    pub owned: Labels,
    pub domains: Vec<Domain>,
    pub top: Limits,
}

/// The fixed part of the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixed {
    pub principals: Vec<Principal>,
    pub keyd: BTreeSet<u64>,
    pub servers: u16,
    pub sizes: Sizes,
}

/// What boot carves for one principal: its top budget, and a fixed sub-budget per label set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carve {
    pub account: NonZeroU64,
    pub top: Limits,
    pub subs: Vec<(Domain, Limits)>,
}

impl Fixed {
    /// The manifest, checked: accounts non-zero and distinct, names distinct, label sets valid
    /// and distinct, and no key in two roles or held by `keyd` (R35).
    pub fn new(m: &Manifest) -> Option<Fixed> {
        let mut accounts = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut logins = BTreeSet::new();
        let mut approvals = BTreeSet::new();
        let mut principals = Vec::new();
        for p in &m.principals {
            let account = NonZeroU64::new(p.account)?;
            if !accounts.insert(account) || !names.insert(p.name.clone()) {
                return None;
            }
            logins.extend(p.login_keys.iter().copied());
            approvals.extend(p.approval_keys.iter().copied());
            let mut domains: Vec<Domain> = Vec::new();
            for set in &p.label_sets {
                let d = Domain::new(account, Labels::new(set)?);
                if domains.contains(&d) {
                    return None;
                }
                domains.push(d);
            }
            let principal = Principal {
                name: p.name.clone(),
                account,
                login_keys: p.login_keys.clone(),
                approval_keys: p.approval_keys.clone(),
                owned: Labels::new(&p.owned)?,
                domains,
                top: p.top,
            };
            principals.push(principal);
        }
        let keyd: BTreeSet<u64> = m.keyd_keys.iter().copied().collect();
        if logins.intersection(&approvals).next().is_some()
            || keyd.iter().any(|k| logins.contains(k) || approvals.contains(k))
        {
            return None;
        }
        Some(Fixed { principals, keyd, servers: m.servers, sizes: m.sizes })
    }

    pub fn principal(&self, name: &str) -> Option<usize> {
        self.principals.iter().position(|p| p.name == name)
    }

    /// The principal whose account this is.
    pub fn by_account(&self, account: NonZeroU64) -> Option<usize> {
        self.principals.iter().position(|p| p.account == account)
    }

    /// Boot's carving: an equal share of the top budget per label set, less each sub-budget's own
    /// object.
    pub fn carves(&self) -> Vec<Carve> {
        let cost = self.sizes.budget_cost;
        self.principals
            .iter()
            .map(|p| {
                let n = (p.domains.len() as u64).max(1);
                let share = Limits {
                    pages: (p.top.pages / n).saturating_sub(cost),
                    processes: p.top.processes / n,
                    weight: p.top.weight / n,
                };
                Carve {
                    account: p.account,
                    top: p.top,
                    subs: p.domains.iter().map(|d| (d.clone(), share)).collect(),
                }
            })
            .collect()
    }
}
