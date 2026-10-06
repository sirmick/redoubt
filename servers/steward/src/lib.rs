//! The steward server (servers/steward.md): it embeds the policy core (`redoubt-steward`) and
//! binds its effects to the kernel and the client library. It decides nothing itself: the core's
//! `decide` does, and the server carries out what it names, in order.
//!
//! At its start: the manifest lines from the arguments (servers/steward.md, "The manifest
//! lines"), the core's boot, and boot's carve as kernel calls: for each principal a top budget
//! under `users` with its account and limits, and under it one fixed sub-budget per label set,
//! with that set's labels ("Fixed sub-budgets per label set"). A failed carve is a start failure:
//! the box has no users. Then each event through the core and its batches ([`drive`]), and the
//! protocol's calls by badge class ([`protocol`]).
//!
//! Everything it does is here, behind [`Kernel`], so host tests drive the same code
//! (`tests/steward.rs`).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod drive;
pub mod own;
pub mod protocol;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use redoubt_rt::abi::{BudgetSpec, Error, FOREVER, Labels};
use redoubt_steward::domain::Domain;
use redoubt_steward::effect::{Object, Step};
use redoubt_steward::manifest::{Limits, Lines, Sizes};
use redoubt_steward::{Policy, Store};

use crate::drive::MadeMap;
use crate::own::{Own, is_own};

/// The shared servers a session's namespace holds a connection to, one `Shared` slot each in the
/// binding table, in this order: `bootfsd` at `/boot`, the home volume's `littlefsd`, the
/// labelled volume's `littlefsd` at `/vault`, `ipd` at `/net`, the console at `/dev/cons`, and
/// the system volume's `erofsd` (servers/steward.md, "Two embedders and a reference"). The manifest's
/// `servers` line must say this many.
pub const SLOTS: u16 = 6;

/// The kernel and client calls the steward makes, so host tests can stand in for them. The
/// binding table (which server each `Shared` slot is, for which domain, and where the child finds
/// it) is the machine's, behind `connect` and `launch`.
pub trait Kernel {
    /// A budget handle.
    type Budget: Copy;
    /// A connection or badge handle the steward holds for a child.
    type Handle: Copy;
    /// `budget_create` under `parent`.
    fn create(&mut self, parent: Self::Budget, spec: &BudgetSpec) -> Result<Self::Budget, Error>;
    /// Whether `budget` holds nothing: no pages charged, no process, no child.
    fn empty(&mut self, budget: Self::Budget) -> Result<bool, Error>;
    /// `budget_destroy`: everything charged to it ends.
    fn destroy(&mut self, budget: Self::Budget) -> Result<(), Error>;
    /// The id audit records name the budget by.
    fn budget_id(&self, budget: Self::Budget) -> u64;
    /// A badge on the steward's own endpoint, stamped with `stamp`.
    fn mint(&mut self, badge: u64, stamp: Self::Budget) -> Result<Self::Handle, Error>;
    /// A fresh connection for a child of `domain` at shared slot `slot`, or none where the slot
    /// binds to nothing for that domain.
    fn connect(&mut self, domain: &Domain, slot: u16) -> Result<Option<Self::Handle>, Error>;
    /// The console of the next login's session: the channel's connection `sshd` sent with it,
    /// which the console slot binds to; the steward owns it from here.
    fn console(&mut self, handle: Option<redoubt_rt::abi::Handle>);
    /// Gives a connection back: closed here, and disconnected at its server.
    fn release(&mut self, handle: Self::Handle);
    /// Starts a child of `domain` in `budget` through the loader stub, with the connections the
    /// batch made in slot order (the steward's first); returns its process id.
    fn launch(
        &mut self,
        domain: &Domain,
        budget: Self::Budget,
        connections: &[Option<Self::Handle>],
    ) -> Result<u64, Error>;
    /// A word from the kernel's generator.
    fn random(&mut self) -> Result<u64, Error>;
    /// Microseconds since boot.
    fn now(&mut self) -> u64;
}

/// Why the steward did not start: it says it once and exits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    /// A manifest line the parser refused, with why.
    Lines(String),
    /// `users` was not empty: a steward restarted without `init` recreating it (servers/init.md,
    /// "Restarts and reboots") would carve a second set beside the first.
    UsersNotEmpty,
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
            StartError::UsersNotEmpty => write!(f, "steward: users not empty"),
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

/// The running steward: the core's store, the budgets boot carved, and what each batch made.
pub struct Steward<B, H> {
    pub store: Store,
    pub users: B,
    pub carved: Vec<Carved<B>>,
    /// What the steward's own lines say: label names, homes, vaults and network scopes.
    pub own: Own,
    /// The sizes the core carves, which a session's arguments repeat.
    pub sizes: Sizes,
    made: MadeMap<B, H>,
    /// Each running process, by id, and the object it is.
    pids: BTreeMap<u64, Object>,
    /// The console principal's session, while it runs.
    console_session: Option<u64>,
    /// Each step that failed since the server last said them, with the kernel's error.
    pub failed: Vec<(Step, Error)>,
}

impl<B: Copy, H> Steward<B, H> {
    /// The label ids a login's label names: none for `""`, the manifest's id for a label it
    /// names, `None` for any other.
    pub fn label(&self, name: &str) -> Option<Vec<u64>> {
        if name.is_empty() {
            return Some(Vec::new());
        }
        if !redoubt_rt::startup::valid_name(name) {
            return None;
        }
        self.own.labels.iter().find(|(n, _)| n == name).map(|(_, id)| alloc::vec![*id])
    }

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
) -> Result<Steward<K::Budget, K::Handle>, StartError> {
    // The core's lines and the steward's own, numbered together from 1 as `init` wrote them.
    let (mut core, mut own) = (Lines::default(), Own::default());
    let mut console_line = 0;
    for (i, line) in lines.iter().enumerate() {
        let read = if is_own(line) { own.line(line) } else { core.line(line) };
        read.map_err(|e| StartError::Lines(alloc::format!("line {}: {e}", i + 1)))?;
        if line.starts_with("console ") {
            console_line = i + 1;
        }
    }
    let manifest = core.finish().map_err(StartError::Lines)?;
    // init refuses a console that is no principal before any server starts; this is defence in
    // depth, naming the line.
    if let Some(c) = &own.console {
        if !manifest.principals.iter().any(|p| &p.name == c) {
            return Err(StartError::Lines(alloc::format!("line {console_line}: {c} is no principal")));
        }
    }
    if manifest.servers != SLOTS {
        return Err(StartError::Slots(manifest.servers));
    }
    let (store, carves) = Store::boot(&manifest, Policy::SHIPPED).ok_or(StartError::Manifest)?;
    // `users` that cannot be read is not known to be empty: the same failure.
    if !kernel.empty(users).unwrap_or(false) {
        return Err(StartError::UsersNotEmpty);
    }
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
    let sizes = manifest.sizes;
    Ok(Steward {
        store,
        users,
        carved,
        own,
        sizes,
        made: BTreeMap::new(),
        pids: BTreeMap::new(),
        console_session: None,
        failed: Vec::new(),
    })
}
