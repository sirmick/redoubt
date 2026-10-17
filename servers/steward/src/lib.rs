//! The steward server (servers/steward.md): it embeds the policy core (`redoubt-steward`) and
//! binds its effects to the kernel and the client library. It decides nothing itself: the core's
//! `decide` does, and the server carries out what it names, in order.
//!
//! At its start: the manifest lines from the arguments (servers/steward.md, "The manifest
//! lines"), the core's boot, and boot's carve as kernel calls: for each principal a top budget
//! under `users` with its account and limits, and under it one fixed sub-budget per label set,
//! with that set's labels ("Fixed sub-budgets per label set"). A failed carve is a start failure:
//! the box has no users. Then each event through the core and its batches ([`drive`]), and the
//! protocol's calls by badge class ([`protocol`]); the threads that watch sessions' exits are
//! reused ([`watchers`]).
//!
//! Everything it does is here, behind [`Kernel`], so host tests drive the same code
//! (`tests/steward.rs`).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod approval;
pub mod cleanup;
pub mod console;
pub mod drive;
#[cfg(feature = "fuzz")]
pub mod fuzz;
pub mod own;
pub mod protocol;
pub mod watchers;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use redoubt_rt::abi::{BudgetSpec, Error, FOREVER};
pub use redoubt_steward::consts::SLOTS;
use redoubt_steward::domain::Domain;
use redoubt_steward::effect::{Kind, Object, Step};
use redoubt_steward::manifest::{Lines, Sizes};
use redoubt_steward::{Policy, Store};

use crate::drive::{MadeMap, spec};
use crate::own::{Own, is_own};

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
    /// A fresh connection for `owner`'s child (a session's or a lease's: the binding table
    /// differs by kind) at shared slot `slot`, minted in the revocation scope `scope` so that
    /// destroying the scope ends it (servers/steward.md R41), or none where the slot binds to
    /// nothing for it.
    fn connect(
        &mut self,
        owner: &Object,
        slot: u16,
        scope: Self::Budget,
    ) -> Result<Option<Self::Handle>, Error>;
    /// The consoles of the next login's session, the two connections to the channel `sshd` sent
    /// with it: the steward's own, through which `ended` ends the channel, and the relay's, which
    /// `attach` hands on; the steward owns both from here.
    fn console(&mut self, console: Option<redoubt_rt::abi::Handle>, relay: Option<redoubt_rt::abi::Handle>);
    /// A connection is given back, the steward's side: its own copy of a handle minted in the
    /// owner's scope is closed now, before the scope's end frees the index for the next handle
    /// installed. The server's side is [`Kernel::clean`]'s, deferred to the loop's turn
    /// ([`cleanup`]).
    fn release(&mut self, handle: Self::Handle);
    /// The server is told to free what it held for the connection (`disconnect` by its id, or
    /// `ended` for `sshd`'s channel console: servers/steward.md, "Failure and restart"), within
    /// the steward's own send bound, never the call's. `Err(Timeout)` when the server did not
    /// take the call in time: the record is kept for the retry, and when `parks` (the attempt
    /// past which the step waits for the server's next answer: [`cleanup`]) the console says
    /// so, once. Any other outcome ends the step: a connection whose server restarted since has
    /// no record left and nothing to tell.
    fn clean(&mut self, handle: Self::Handle, parks: bool) -> Result<(), Error>;
    /// The server [`Kernel::clean`] tells for `handle`, by the name the steward reaches it
    /// under (`consoled` for a console it minted, `sshd` for the channel console); none for a
    /// connection nobody is told.
    fn server_of(&self, handle: Self::Handle) -> Option<String>;
    /// The server a connection for `owner`'s child at `slot` is minted at ([`Kernel::connect`],
    /// [`Kernel::rebind`]), or its domain's vault's for `None` (a crossing's: [`Kernel::cross`]),
    /// by the same name; none where the slot binds nothing minted at a server.
    fn server_at(&self, owner: &Object, slot: Option<u16>) -> Option<String>;
    /// Starts `owner`'s child in `budget` through the loader stub (a session's VM, or an agent's
    /// for a lease), with the connections the batch made in slot order (the steward's first),
    /// told the session's `context` (`None` for the console's session or an agent); returns its
    /// process id and, for a session, the send right of the `notices` endpoint its VM was given
    /// the receive right of, which [`Kernel::notify`] sends on (servers/steward.md, "Leases").
    fn launch(
        &mut self,
        owner: &Object,
        budget: Self::Budget,
        connections: &[Option<Self::Handle>],
        context: Option<&str>,
    ) -> Result<(u64, Option<Self::Handle>), Error>;
    /// Sends `lease_ended` for `lease`, named `name`, on a session's notices endpoint: one way,
    /// bounded, never retried. A send that does not go is dropped: the steward is never held by
    /// a session that does not read.
    fn notify(&mut self, notices: Self::Handle, lease: u64, name: &str);
    /// Sends `approval_waiting` on a session's notices endpoint, the same way.
    fn notify_approval(&mut self, notices: Self::Handle);
    /// The consoles of the next approval channel, the two connections to the `approve@box`
    /// channel `sshd` sent with `approval_opened` (servers/steward.md, "The powerbox and
    /// approvals"): the one the steward writes screens through, and the one its reader for the
    /// channel reads the person's lines through; the steward owns both from here, and
    /// [`Kernel::channel_opened`] takes them or the next call drops them.
    fn approval_consoles(
        &mut self,
        console: Option<redoubt_rt::abi::Handle>,
        reader: Option<redoubt_rt::abi::Handle>,
    );
    /// The core opened approval channel `channel`: the consoles of
    /// [`Kernel::approval_consoles`] are its, and its reader starts; `Err` when the steward
    /// cannot serve it (the channel cap, no thread), and the core is told it closed.
    fn channel_opened(&mut self, channel: u64) -> Result<(), Error>;
    /// Writes `text`, printable ASCII and line ends, on `channel`'s console: bounded, one way; a
    /// write that does not go within the bound is dropped, and the person asks again with
    /// `pending`. The steward is never held by a terminal that does not read.
    fn screen(&mut self, channel: u64, text: &str);
    /// Approval channel `channel` is closed: its consoles go, and its reader ends at the end of
    /// its input.
    fn channel_closed(&mut self, channel: u64);
    /// `init` re-made the steward's connection at `server`, restarted (servers/steward.md,
    /// "Failure and restart"): the steward reaches the server by `connection` from here, and
    /// what it held there (its attached connection, the carves it kept, every child's
    /// connection) died with the old instance and is forgotten, nothing disconnected.
    fn reconnected(&mut self, server: &str, connection: redoubt_rt::abi::Handle);
    /// The slots of `owner`'s binding table at `server` whose connection its child binds at a
    /// path: the ones it is told to re-bind after `server`'s restart. A named handle (the
    /// system volume's) and the console are not among them.
    fn slots_at(&self, owner: &Object, server: &str) -> Vec<u16>;
    /// Sends `server_restarted` for `server`'s `slot` on a session's notices endpoint, as
    /// [`Kernel::notify`] sends: one way, bounded, never retried.
    fn restarted(&mut self, notices: Self::Handle, server: &str, slot: u16);
    /// A fresh connection for `owner`'s child at `slot` again, in `scope`, after the slot's
    /// server restarted: the handle it is held under, the kernel handle the reply carries to the
    /// child (the steward's own copy goes with the reply), and the path the child binds it at;
    /// none where the slot binds nothing at a path.
    fn rebind(
        &mut self,
        owner: &Object,
        slot: u16,
        scope: Self::Budget,
    ) -> Result<Option<(Self::Handle, redoubt_rt::abi::Handle, String)>, Error>;
    /// A context's console relay, started in `budget` (servers/steward.md, "Contexts"): its
    /// process id, and the steward's control connection to it. The next console connection of
    /// `domain`'s namespace is the relay's, which it serves the session's `/dev/cons` on.
    fn launch_relay(&mut self, domain: &Domain, budget: Self::Budget) -> Result<(u64, Self::Handle), Error>;
    /// The login's channel console, kept, and the relay's connection to it given to `relay` with
    /// `note`, which the relay writes there first; returns the console kept.
    fn attach(&mut self, relay: Self::Handle, note: &str) -> Result<Self::Handle, Error>;
    /// `relay` writes `note` to its channel and lets it go.
    fn detach(&mut self, relay: Self::Handle, note: &str) -> Result<(), Error>;
    /// A crossing's one item (servers/steward.md, "Declassification and push"): the `crossing`
    /// program launched in `budget`, which carries exactly `owner`'s labels, with one connection
    /// to `owner`'s domain's vault at its root, minted in `budget` as its scope (R41), and
    /// called once: `read` for the item at `path`, answered with its bytes, or `write` of `bytes`
    /// as it (`None` reads). The program, labelled, walks to the item: the steward, unlabelled,
    /// may mint at the vault's root alone, since a mint below it walks the path with the
    /// caller's labels (R25). Returns the process id, the key the connection is held under (given
    /// back after the budget is destroyed: the cleanup, never the cutoff) and the bytes read.
    fn cross(
        &mut self,
        owner: &Object,
        budget: Self::Budget,
        path: &str,
        bytes: Option<&[u8]>,
    ) -> Result<(u64, Option<Self::Handle>, Vec<u8>), Error>;
    /// The length of `path` in `principal`'s home, through the steward's own kept connection
    /// there; `None` for no such file.
    fn home_stat(&mut self, principal: &str, path: &str) -> Result<Option<u64>, Error>;
    /// `path` in `principal`'s home, read whole by the steward itself: at most `max` bytes, an
    /// error past them.
    fn home_read(&mut self, principal: &str, path: &str, max: usize) -> Result<Vec<u8>, Error>;
    /// `bytes` written by the steward itself as the new file `path` in `principal`'s home, its
    /// directory made if missing; never over a file that is there.
    fn home_write(&mut self, principal: &str, path: &str, bytes: &[u8]) -> Result<(), Error>;
    /// A word from the kernel's generator.
    fn random(&mut self) -> Result<u64, Error>;
    /// Microseconds since boot.
    fn now(&mut self) -> u64;
}

/// The most a push moves (servers/steward.md, "Declassification and push"): fifteen pages of the
/// steward's sixteen-page lend, the sixteenth for the item's name and the message's lengths, so
/// the snapshot travels to the writer in one call.
pub const PUSH_MAX: usize = 15 * 4096;

/// An item a request names by an id the server drew: the path it stands for, in the vault of the
/// request's labelled side (a declassified item, a push's target) or in the submitter's home (a
/// push's source), kept for the request's life. The path is the requester's text, shown on the
/// screen beside the core's line and never part of the binding hash, which covers the ids and the
/// snapshot: what is approved is the bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The request, once the core has drawn its id; none while its submission is decided.
    pub request: Option<Object>,
    pub path: String,
}

/// Why the steward did not start: it says it once and exits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    /// A manifest line the parser refused, with why.
    Lines(String),
    /// `users` was not empty: a steward restarted without `init` emptying it first
    /// (servers/init.md, "Restarts and reboots") would carve a second set beside the first.
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
    /// The connections given back whose servers are still to be told ([`cleanup`]).
    cleanup: cleanup::Queue<H>,
    /// Each running process, by id, and the object it is.
    pids: BTreeMap<u64, Object>,
    /// Each process whose object the core has let go of before the process's exit notice came
    /// (a lease ended by `end_lease`, a session its deadline took): the object and the name its
    /// notice gave it, for the exit line that notice correlates with; taken by [`Steward::ended`].
    ended: BTreeMap<u64, (Object, String)>,
    /// Each session's notices endpoint: the send right its launch kept, closed with the session.
    notices: BTreeMap<Object, H>,
    /// The last screen written on each open approval channel: each request shown, by id, with
    /// the binding hash it showed, so a person's 4-byte code expands to the hash of the screen
    /// they saw and nothing else (servers/steward.md, "The powerbox and approvals").
    screens: BTreeMap<u64, Vec<(u64, [u8; 32])>>,
    /// The console principal's session, while it runs.
    console_session: Option<u64>,
    /// Each step that failed since the server last said them, with the kernel's error.
    pub failed: Vec<(Step, Error)>,
    /// Each item a pending request names, by the id the server drew for it.
    items: BTreeMap<u64, Item>,
}

impl<B: Copy, H> Steward<B, H> {
    /// The name of the principal whose account is `account`, if the manifest has one.
    pub fn principal_name(&self, account: u64) -> Option<String> {
        redoubt_steward::inspect::fixed(&self.store)
            .principals
            .iter()
            .find(|p| p.account.get() == account)
            .map(|p| p.name.clone())
    }

    /// The account of the session or lease routed by `badge`.
    pub fn account_of(&self, badge: u64) -> Option<u64> {
        redoubt_steward::inspect::index(&self.store).routes.get(&badge).map(|r| r.domain.account().get())
    }

    /// Keeps `path` as item `id`'s, for the request being submitted.
    pub fn item(&mut self, id: u64, path: String) { self.items.insert(id, Item { request: None, path }); }

    /// The items of the submission being decided are request `request`'s, now that the core has
    /// drawn its id and holds it in its domain.
    fn items_of_request(&mut self, request: u64) {
        let Some(domain) = redoubt_steward::inspect::domains(&self.store)
            .find(|(_, s)| s.requests.contains_key(&request))
            .map(|(d, _)| d.clone())
        else {
            return;
        };
        let object = Object { domain, kind: Kind::Request, id: request };
        for i in self.items.values_mut().filter(|i| i.request.is_none()) {
            i.request = Some(object.clone());
        }
    }

    /// Drops the items of requests the core no longer holds (answered, denied, dropped with
    /// their session, or refused at submission): the tables emit no `forget` for a request, so
    /// the server reads the store for what is gone.
    fn sweep_items(&mut self) {
        let store = &self.store;
        self.items.retain(|_, i| {
            i.request.as_ref().is_some_and(|o| {
                redoubt_steward::inspect::domain(store, &o.domain).is_some_and(|s| s.has(Kind::Request, o.id))
            })
        });
    }

    /// The items request `request` names, by id, with their paths.
    pub fn items_named(&self, request: u64) -> Vec<(u64, &str)> {
        self.items
            .iter()
            .filter(|(_, i)| i.request.as_ref().is_some_and(|o| o.id == request))
            .map(|(id, i)| (*id, i.path.as_str()))
            .collect()
    }

    /// How many items pending requests name.
    pub fn items_held(&self) -> usize { self.items.len() }

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

    /// How many things the steps made are held: budgets, scopes, connections and processes of
    /// every object the core still knows. What an object's end leaves is nothing of its own.
    pub fn holding(&self) -> usize { self.made.len() }

    /// How many connections given back still wait for their server to be told, parked ones
    /// included.
    pub fn pending(&self) -> usize { self.cleanup.len() }

    /// How many of those are parked: their server took none of [`cleanup::TRIES`] attempts, and
    /// they wait for its next answer or its restart.
    pub fn parked(&self) -> usize { self.cleanup.parked() }
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
    // Boot carves for each unsponsored principal in the manifest's order; a sponsored one's
    // sessions are carved under its sponsor's sub-budget, so it has no top of its own.
    for c in &carves {
        let account = c.account.get();
        let refused = |labels: &[u64], error| StartError::Carve { account, labels: labels.to_vec(), error };
        // The account is set once, here: everything under the top budget shares it (R8, R14).
        let top_spec = spec(&c.top, &[], account, FOREVER).map_err(|_| StartError::Limits { account })?;
        let top = kernel.create(users, &top_spec).map_err(|e| refused(&[], e))?;
        let mut subs = Vec::new();
        for (domain, limits) in &c.subs {
            let labels = domain.labels().as_slice();
            let sub_spec = spec(limits, labels, 0, FOREVER).map_err(|_| StartError::Limits { account })?;
            let sub = kernel.create(top, &sub_spec).map_err(|e| refused(labels, e))?;
            subs.push((domain.clone(), sub));
        }
        carved.push(Carved { name: c.name.clone(), account, top, subs });
    }
    let sizes = manifest.sizes;
    Ok(Steward {
        store,
        users,
        carved,
        own,
        sizes,
        made: BTreeMap::new(),
        cleanup: cleanup::Queue::default(),
        pids: BTreeMap::new(),
        ended: BTreeMap::new(),
        notices: BTreeMap::new(),
        screens: BTreeMap::new(),
        console_session: None,
        failed: Vec::new(),
        items: BTreeMap::new(),
    })
}
