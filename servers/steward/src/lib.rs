//! The steward server (servers/steward.md): it embeds the policy core (`redoubt-steward`) and
//! binds its effects to the kernel and the client library. It decides nothing itself: the core's
//! `decide` does, and the server carries out what it names, in order.
//!
//! This step: the manifest lines from the arguments (servers/steward.md, "The manifest lines"),
//! the core's boot, and boot's carve as kernel calls: for each principal a top budget under
//! `users` with its account and limits, and under it one fixed sub-budget per label set, with
//! that set's labels ("Fixed sub-budgets per label set"). A failed carve is a start failure: the
//! box has no users.
//!
//! Everything it does is here, behind [`Kernel`], so host tests drive the same code
//! (`tests/steward.rs`).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use redoubt_rt::abi::{BudgetSpec, Error, FOREVER, Labels};
use redoubt_steward::domain::Domain;
use redoubt_steward::manifest::{Limits, parse_lines};
use redoubt_steward::{Policy, Store};

/// The shared servers a session's namespace holds a connection to, one `Shared` slot each in the
/// binding table, in this order: `bootfsd` at `/boot`, the home volume's `fsd`, the labelled
/// volume's `fsd` at `/vault`, `ipd` at `/net`, and the console at `/dev/cons`
/// (servers/steward.md, "Two embedders and a reference"). The manifest's `servers` line must say
/// this many.
pub const SLOTS: u16 = 5;

/// The kernel calls the steward makes, so host tests can stand in for them.
pub trait Kernel {
    /// A budget handle.
    type Budget: Copy;
    /// `budget_create` under `parent`.
    fn create(&mut self, parent: Self::Budget, spec: &BudgetSpec) -> Result<Self::Budget, Error>;
}

/// Why the steward did not start: it says it once and exits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    /// A manifest line the parser refused, with why.
    Lines(String),
    /// A `servers` line other than [`SLOTS`]: the core would connect a session to slots the
    /// binding table does not have, or leave some out.
    Slots(u16),
    /// The core refused the manifest (`Fixed::new`): an account 0 or twice, a name twice, a
    /// label set twice or over the kernel's count, or a key in two roles or held by `keyd`.
    Manifest,
    /// Limits the kernel cannot take: processes or weight over `u32::MAX`.
    Limits { account: u64 },
    /// The kernel refused a carve: the budget's account, its labels (empty for the top budget)
    /// and the kernel's error.
    Carve { account: u64, labels: Vec<u64>, error: Error },
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::Lines(why) => write!(f, "steward: a manifest line is refused: {why}"),
            StartError::Slots(n) => write!(f, "steward: {n} servers, and the binding table has {SLOTS}"),
            StartError::Manifest => write!(f, "steward: the manifest is refused"),
            StartError::Limits { account } => write!(f, "steward: account {account}'s limits are too large"),
            StartError::Carve { account, labels, error } => {
                write!(f, "steward: cannot carve account {account} {labels:?}: {error:?}")
            }
        }
    }
}

/// A principal's budgets as boot carved them: its top budget, and each domain's sub-budget.
pub struct Carved<B> {
    pub name: String,
    pub account: u64,
    pub top: B,
    pub subs: Vec<(Domain, B)>,
}

/// The running steward: the core's store, and the budgets boot carved.
pub struct Steward<B> {
    pub store: Store,
    pub carved: Vec<Carved<B>>,
}

impl<B: Copy> Steward<B> {
    /// The fixed sub-budget of `domain`, which every session and lease of it is carved from.
    pub fn sub(&self, domain: &Domain) -> Option<B> {
        self.carved.iter().flat_map(|c| &c.subs).find(|(d, _)| d == domain).map(|(_, b)| *b)
    }
}

fn spec(l: &Limits, labels: &[u64], account: u64) -> Option<BudgetSpec> {
    Some(BudgetSpec {
        pages: l.pages,
        processes: u32::try_from(l.processes).ok()?,
        weight: u32::try_from(l.weight).ok()?,
        labels: Labels::from_slice(labels).ok()?,
        account,
        deadline: FOREVER,
    })
}

/// Starts the steward from its manifest lines: the core's boot, then its carve under `users`.
pub fn start<K: Kernel>(
    lines: &[&str],
    users: K::Budget,
    kernel: &mut K,
) -> Result<Steward<K::Budget>, StartError> {
    let manifest = parse_lines(lines.iter().copied()).map_err(StartError::Lines)?;
    if manifest.servers != SLOTS {
        return Err(StartError::Slots(manifest.servers));
    }
    let (store, carves) = Store::boot(&manifest, Policy::SHIPPED).ok_or(StartError::Manifest)?;
    let mut carved = Vec::new();
    // Boot carves for each principal in the manifest's order.
    for (c, p) in carves.iter().zip(&manifest.principals) {
        let account = c.account.get();
        let refused = |labels: &[u64], error| StartError::Carve { account, labels: labels.to_vec(), error };
        // The account is set once, here: everything under the top budget shares it (R8, R14).
        let top_spec = spec(&c.top, &[], account).ok_or(StartError::Limits { account })?;
        let top = kernel.create(users, &top_spec).map_err(|e| refused(&[], e))?;
        let mut subs = Vec::new();
        for (domain, limits) in &c.subs {
            let labels = domain.labels().as_slice();
            let sub_spec = spec(limits, labels, 0).ok_or(StartError::Limits { account })?;
            let sub = kernel.create(top, &sub_spec).map_err(|e| refused(labels, e))?;
            subs.push((domain.clone(), sub));
        }
        carved.push(Carved { name: p.name.clone(), account, top, subs });
    }
    Ok(Steward { store, carved })
}
