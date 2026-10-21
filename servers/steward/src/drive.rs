//! The core driven (servers/steward.md, "One decision function"): each event goes through
//! `decide`, each batch it names is run step by step against the [`Kernel`], stopping at the
//! first failure, and its outcome goes back in as one `Done` event, until nothing is left to run.
//! The server decides nothing here: it carries out the steps in order and keeps what they made
//! under the core's tokens.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_rt::abi::{BudgetSpec, Error, FOREVER, Handle, Labels};
use redoubt_steward::consts::RANDOM_WORDS;
use redoubt_steward::effect::{
    Answer, Batch, Bytes, Kind, Notice, Notified, Object, Output, Parent, Produced, Rendered, ReplyTo,
    Server, Step, StepFailed, Token,
};
use redoubt_steward::effects::{CONNECTIONS, SCOPE};
use redoubt_steward::event::{Event, EventKind};
use redoubt_steward::manifest::Limits;
use redoubt_steward::{Effects, decide, inspect};

use crate::{Kernel, Launched, PUSH_MAX, Steward};

/// The slot of a crossing's one connection among its owner's tokens: given back after the
/// crossing's budget is destroyed, as a session's are (R41's order).
const CROSSING_CONNECTION: u8 = 3;

/// The directory a declassified item lands in, below the submitter's home: `~/out/<name>`,
/// `<name>` the item's last path element (servers/steward.md, "Declassification and push").
pub const OUT: &str = "out";

/// Every word the server hands the core has this bit set, so each id and badge the core draws
/// from them is in the minted range (servers/serving.md R27): a session's badge can never be a
/// root badge, whatever the generator gave. 63 random bits remain (R36).
pub const MINTED: u64 = 1 << 63;

/// The reply token of the console session's event: its answer names the session the steward
/// reopens when it ends. A protocol call's token is 1 for a host test's one call at a time; the
/// server draws its calls' tokens above this one, since several may wait at once.
pub const CONSOLE: ReplyTo = 2;

/// What a step made, kept under its token until the core forgets its owner.
pub enum Made<B, H> {
    /// A budget. Past its deadline the kernel has destroyed it, and the handle is a dead entry
    /// until the steward closes it (R10, I17; [`Steward::destroy`]).
    Budget(B),
    /// A revocation scope, with the token of the budget it was made in: minted with no deadline
    /// of its own, it ends with that budget, and its handle is then a dead entry, closed with
    /// that budget's record ([`Steward::end_scopes`]).
    Scope(B, Token),
    /// A connection the session's namespace holds, or none for a slot bound to nothing.
    Connection(Option<H>),
    Process(u64),
    /// A context's relay: its process, and the steward's control connection to it.
    Relay(u64, H),
}

/// The steward exited: the core met an event its embedder's guarantee excludes, and the server
/// fails closed (`init` restarts it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exited;

/// A batch stopped at its `Launch` step while the launch thread runs the launch
/// (servers/steward.md, "Leases"): the batch, the step's index, what the steps before it
/// produced, and the thread's job. [`Steward::launched`] finishes it when the thread reports.
pub struct Pending {
    batch: Batch,
    step: usize,
    produced: Vec<Produced>,
    job: u64,
}

impl Pending {
    /// Whether the batch carves a budget from `domain`'s fixed sub-budget: its own domain's for
    /// a session or a lease, the sponsor's for a sponsored principal's session (R39).
    pub(crate) fn carves_from(&self, domain: &redoubt_steward::domain::Domain) -> bool {
        self.batch
            .steps
            .iter()
            .any(|s| matches!(s, Step::CreateBudget { parent: Parent::Sub(d), .. } if d == domain))
    }
}

/// An event held for a launching object, with its reply token.
pub type Held = (EventKind, ReplyTo);

/// How a batch's run ended: with its outcome, the core's `Done`; or stopped at a launch the
/// thread runs, its outcome a later event.
pub enum Ran {
    Done(Result<Vec<Produced>, StepFailed>),
    Pending(Pending),
}

/// What one step did: what it made, or a launch handed to the thread as this job.
enum Stepped {
    Made(Produced),
    Launching(u64),
}

impl<B: Copy, H: Copy> Steward<B, H> {
    /// Decides `kind`, with `reply` the caller's token (0 for none), runs every batch that follows
    /// and returns the outputs for the server to deliver: replies, notices, screens, audit
    /// records. What the core forgets is dropped here. A batch stopped at its launch is kept
    /// for the launch thread's report, its `Done` and its caller's reply coming then
    /// ([`Steward::launched`]); an event of an object whose batch waits is held until then
    /// ([`Steward::holds`]) and the caller parked, its token among [`Steward::awaits`].
    pub fn event<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        kind: EventKind,
        reply: ReplyTo,
    ) -> Result<Vec<Output>, Exited> {
        self.events(kernel, alloc::vec![(kind, reply)])
    }

    /// The `Exited` events of processes whose exit notices arrived together, as one batch: every
    /// exit is decided and its sponsor's `lease_ended` notice sent before any exit's bookkeeping
    /// runs (the end batch and the `Done` it raises, the forgets, the records delivered after),
    /// so the last notice of a burst of N waits behind N - 1 sends, not N - 1 exits' work
    /// (servers/steward.md, "Failure and restart"). The core sees what it would one exit at a
    /// time: an `Exited` moves its object to `Ending` by its own row, and the `Done` that ends it
    /// follows its own batch; the other exits' decisions between them are other objects' rows,
    /// which read nothing of it. One object's exit is [`Steward::event`]'s.
    pub fn exits<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        objects: Vec<Object>,
    ) -> Result<Vec<Output>, Exited> {
        self.events(kernel, objects.into_iter().map(|object| (EventKind::Exited { object }, 0)).collect())
    }

    /// `heads` decided in order, each sending its notices as it is decided; then each head's
    /// effects run in order with every event that follows from them (its batches' `Done`s, the
    /// `Pending` of a channel it redraws), decided as they come to the front, before the next
    /// head's. A batch of one is one event.
    fn events<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        heads: Vec<(EventKind, ReplyTo)>,
    ) -> Result<Vec<Output>, Exited> {
        let mut outputs = Vec::new();
        let mut reopen = false;
        // The approval channels these events redraw (a `Pending` of theirs ran), and the screens
        // the core rendered on each: written as one block per channel at the end.
        let mut redrawn = BTreeSet::new();
        let mut screens: BTreeMap<u64, Vec<Rendered>> = BTreeMap::new();
        let mut decided = Vec::with_capacity(heads.len());
        for (kind, reply) in heads {
            // A head of an object whose batch waits at its launch is held for the launch
            // thread's report, its caller parked ([`Steward::holds`]); the rest are decided.
            if self.holds(&kind) {
                if reply != 0 {
                    self.awaiting.insert(reply);
                }
                self.held.push((kind, reply));
                continue;
            }
            decided.push(self.decided(kernel, kind, reply, &mut redrawn)?);
        }
        for effects in decided {
            let mut effects = Some(effects);
            let mut queue = alloc::collections::VecDeque::new();
            loop {
                let Effects { outputs: out, batches, exit: _ } = match effects.take() {
                    Some(e) => e,
                    None => match queue.pop_front() {
                        Some((kind, reply)) => self.decided(kernel, kind, reply, &mut redrawn)?,
                        None => break,
                    },
                };
                for o in out {
                    // The core answered the token: its call waits no longer.
                    if let Output::Reply { to, .. } = &o {
                        self.awaiting.remove(to);
                    }
                    match o {
                        Output::Forget(object) => {
                            reopen |= self.console_session == Some(object.id) && object.kind == Kind::Session;
                            self.forget(kernel, &object);
                        }
                        // A submission is a request now: the items its call drew are the
                        // request's, before any screen names them.
                        Output::Reply { to, answer: Answer::Request { id } } => {
                            self.items_of_request(id);
                            outputs.push(Output::Reply { to, answer: Answer::Request { id } });
                        }
                        Output::Reply { to: CONSOLE, answer } => {
                            self.console_session = match answer {
                                Answer::Session { id, .. } => Some(id),
                                _ => None,
                            };
                            // A console that cannot start is said once, not retried in a loop.
                            if self.console_session.is_none() {
                                outputs.push(Output::Reply { to: CONSOLE, answer });
                            }
                        }
                        Output::Notice { to: Notified::Session(badge), notice: Notice::ApprovalWaiting } => {
                            if let Some(n) = self.notices_of(badge) {
                                kernel.notify_approval(n);
                            }
                        }
                        // An approval channel of the requester's principal is open: it is
                        // redrawn, which renders the request on it (servers/steward.md, "The
                        // powerbox and approvals").
                        Output::Notice {
                            to: Notified::Channel(channel),
                            notice: Notice::ApprovalWaiting,
                        } => {
                            queue.push_back((EventKind::Pending { channel }, 0));
                        }
                        Output::Screen { channel, screen } => {
                            screens.entry(channel).or_default().push(screen)
                        }
                        o => outputs.push(o),
                    }
                }
                for batch in batches {
                    let object = batch.owner.clone();
                    match self.run_from(kernel, batch, 0, Vec::new()) {
                        Ran::Done(result) => queue.push_back((EventKind::Done { object, result }, 0)),
                        Ran::Pending(pending) => self.suspend(pending),
                    }
                }
                // A crossing's final state removes it with no `forget` of the tables': what its
                // steps made (its connection, a budget a failed destroy left) is dropped once the
                // core no longer holds it, after the batches of this decision, so a copy out or a
                // write still finds its item's path; the items of requests gone go the same way.
                let gone: BTreeSet<Object> = self
                    .made
                    .keys()
                    .map(|t| t.owner.clone())
                    .chain(self.pids.values().cloned())
                    .filter(|o| o.kind == Kind::Crossing)
                    .filter(|o| !self.store.domain(&o.domain).is_some_and(|d| d.has(Kind::Crossing, o.id)))
                    .collect();
                for object in gone {
                    self.forget(kernel, &object);
                }
            }
        }
        // A submission's items are bound to its request when the core answers it, which for a
        // declassification or a push is the decision after its read's: the items of requests
        // gone, and of a submission refused, go once the whole event is decided.
        self.sweep_items();
        for channel in redrawn.iter().chain(screens.keys()).copied().collect::<BTreeSet<u64>>() {
            let shown = screens.remove(&channel).unwrap_or_default();
            let text = self.compose(channel, &shown);
            self.screens.insert(channel, shown.iter().map(|r| (r.id, r.hash)).collect());
            kernel.screen(channel, &text);
        }
        if reopen {
            self.console_session = None;
            outputs.extend(self.open_console(kernel)?);
        }
        Ok(outputs)
    }

    /// `kind` decided: the core's effects, less the `lease_ended` notices, which go now, as the
    /// decision is made and ahead of everything else it asks for, while the lease is still in
    /// the store to be named (what the notice says is the lease's name alone). The core asking
    /// for the steward's exit is `Err`, nothing of the decision carried out.
    fn decided<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        kind: EventKind,
        reply: ReplyTo,
        redrawn: &mut BTreeSet<u64>,
    ) -> Result<Effects, Exited> {
        if let EventKind::Pending { channel } = &kind {
            redrawn.insert(*channel);
        }
        // The object's process is gone before the core decides its exit: a destroy the core
        // then asks for may find the budget gone too (a lease the kernel's deadline took).
        if let EventKind::Exited { object } = &kind {
            self.pids.retain(|_, o| o != object);
        }
        let mut random = [0; RANDOM_WORDS];
        for w in &mut random {
            // A generator that fails gives no words, and the core exits rather than number
            // anything (R36).
            *w = kernel.random().map_or(0, |r| r | MINTED);
        }
        let event = Event { now: kernel.now(), random, reply, kind };
        let mut effects = decide(&mut self.store, event);
        if effects.exit {
            return Err(Exited);
        }
        effects.outputs.retain(|o| match o {
            Output::Notice { to: Notified::Session(badge), notice: Notice::LeaseEnded { lease } } => {
                self.notify(kernel, *badge, *lease);
                false
            }
            _ => true,
        });
        Ok(effects)
    }

    /// The screen an approval channel shows (servers/steward.md, "The powerbox and approvals";
    /// sshd.md, "`approve@box`"): a header naming the channel's principal; per request its id,
    /// its binding hash and the hash's first four bytes as the code a person types in its
    /// place, then the core's rendered line (`render.rs`: printable ASCII only, so nothing here
    /// but these bytes and the line ends reaches the terminal), then one line per item the
    /// request names, the path as the requester's text, sanitized and capped as the core
    /// sanitizes a field (the core's line names the item by id alone); and the line to type.
    /// Written as `\r\n` lines: the channel has no line discipline.
    fn compose(&self, channel: u64, shown: &[Rendered]) -> String {
        let index = inspect::index(&self.store);
        let fixed = inspect::fixed(&self.store);
        let who = index
            .channels
            .get(&channel)
            .and_then(|c| fixed.principals.get(c.principal))
            .map_or("nobody", |p| p.name.as_str());
        if shown.is_empty() {
            return format!("no approval is waiting for {who}\r\n");
        }
        let mut text = format!("pending approvals for {who}:\r\n");
        for r in shown {
            let hash: String = r.hash.iter().map(|b| format!("{b:02x}")).collect();
            text.push_str(&format!(
                "request {:016x} hash {hash} code {}\r\n  {}\r\n",
                r.id,
                &hash[..8],
                r.text
            ));
            for (id, path) in self.items_named(r.id) {
                let path = redoubt_steward::render::sanitize(path, redoubt_steward::consts::FIELD_CAP);
                text.push_str(&format!("  item {id}: path (the requester's text) \"{path}\"\r\n"));
            }
        }
        text.push_str("type: approve <id> <hash or code> | deny <id> | pending\r\n");
        text
    }

    /// The hash the last screen on `channel` showed for request `request` with `code` as its
    /// first bytes: the one hash a code can mean, and only while that screen is the last. `None`
    /// for anything else, which is `HashMismatch`, answered to the channel only.
    pub fn hash_of_code(&self, channel: u64, request: u64, code: &[u8]) -> Option<[u8; 32]> {
        self.screens
            .get(&channel)?
            .iter()
            .find(|(id, hash)| *id == request && hash.starts_with(code))
            .map(|(_, hash)| *hash)
    }

    /// Whether a screen was written on `channel`: it is open and was redrawn at least once.
    pub fn screened(&self, channel: u64) -> bool { self.screens.contains_key(&channel) }

    /// Approval channel `channel` is closed: nothing of its screens is kept.
    pub fn channel_closed(&mut self, channel: u64) { self.screens.remove(&channel); }

    /// The notices endpoint of the session routed by `badge`, if the core routes it and it has
    /// one.
    fn notices_of(&self, badge: u64) -> Option<H> {
        let route = inspect::index(&self.store).routes.get(&badge)?;
        let session = Object { domain: route.domain.clone(), kind: route.kind, id: route.id };
        self.notices.get(&session).copied()
    }

    /// Opens the console principal's session, if the manifest names one: at the start, and again
    /// whenever it ends. A refusal comes back as an output to say.
    pub fn open_console<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
    ) -> Result<Vec<Output>, Exited> {
        match self.own.console.clone() {
            Some(principal) => self.event(kernel, EventKind::Console { principal }, CONSOLE),
            None => Ok(Vec::new()),
        }
    }

    /// The object whose process `pid` is, if it still runs: an exit notice for any other is late,
    /// for an object already ending, and is dropped.
    pub fn exited(&self, pid: u64) -> Option<Object> { self.pids.get(&pid).cloned() }

    /// The object whose process `pid` was, and the name its notice gave it, if the core let the
    /// object go before the process's exit notice came (a lease `end_lease` ended, a session its
    /// deadline took): taken once, for the exit line.
    pub fn ended(&mut self, pid: u64) -> Option<(Object, String)> { self.ended.remove(&pid) }

    /// `owner`'s processes leave the running ones for the ended, named now, while the core still
    /// knows the object.
    fn end_pids(&mut self, owner: &Object) {
        let name = self.lease_name(owner.id).unwrap_or_else(|| named(owner));
        let gone: Vec<u64> = self.pids.iter().filter(|(_, o)| *o == owner).map(|(p, _)| *p).collect();
        for pid in gone {
            self.pids.remove(&pid);
            self.ended.insert(pid, (owner.clone(), name.clone()));
        }
    }

    /// A lease's name as a notice says it, `<principal>/<name>`: `alice/agent-3` for an agent
    /// alice started, `agent/session-7` for a sponsored principal's session. `None` for an id
    /// that names nothing now.
    pub fn lease_name(&self, lease: u64) -> Option<String> {
        let index = inspect::index(&self.store);
        let (domain, kind) = index.ids.get(&lease)?;
        let state = inspect::domain(&self.store, domain)?;
        let fixed = inspect::fixed(&self.store);
        let (principal, name) = match kind {
            Kind::Lease => {
                state.leases.get(&lease).map(|l| (l.principal, alloc::format!("agent-{}", l.number)))?
            }
            _ => state.sessions.get(&lease).map(|s| (s.principal, alloc::format!("session-{}", s.number)))?,
        };
        Some(alloc::format!("{}/{name}", fixed.principals.get(principal)?.name))
    }

    /// Gives connection `h` back: the steward's own copy goes now ([`Kernel::release`]), and its
    /// server is told by the cleanup thread ([`Kernel::clean`]), never from inside an event and
    /// never by the loop: the cutoff is the scope's destroy, and a cleanup call a busy or
    /// recovering server does not take would hold whoever makes it for the call's whole bound
    /// (servers/steward.md, "Failure and restart").
    fn give_back<K: Kernel<Budget = B, Handle = H>>(&mut self, kernel: &mut K, h: H) {
        kernel.release(h);
        kernel.clean(h);
    }

    /// A mint at a server with steps parked there wakes them first, since the mint needs the
    /// record a step frees, and the thread runs them meanwhile; a mint the server refuses all
    /// the same fails its step, and the caller is told, as a mint the server does not take in
    /// time is (nothing retries inside the steward: the caller's next attempt finds the record
    /// freed). A mint the server answered, taken or refused, wakes what is still parked there
    /// ([`crate::cleanup`]); a `Timeout` is no answer and wakes nothing. `slot` names the server
    /// as [`Kernel::server_at`] does.
    fn minting<K: Kernel<Budget = B, Handle = H>, T>(
        &mut self,
        kernel: &mut K,
        owner: &Object,
        slot: Option<u16>,
        mint: impl FnOnce(&mut K) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let at = kernel.server_at(owner, slot);
        if let Some(server) = at.as_deref().filter(|server| kernel.parked_at(server)) {
            kernel.wake(server);
        }
        let made = mint(kernel);
        if let Some(server) = at.as_deref() {
            if !matches!(made, Err(Error::Timeout)) {
                kernel.wake(server);
            }
        }
        made
    }

    /// Tells the session routed by `badge` that `lease` ended, on the notices endpoint its launch
    /// kept; a session with none, or one the core no longer routes, is told nothing.
    fn notify<K: Kernel<Budget = B, Handle = H>>(&mut self, kernel: &mut K, badge: u64, lease: u64) {
        let (Some(n), Some(name)) = (self.notices_of(badge), self.lease_name(lease)) else { return };
        kernel.notify(n, lease, &name);
    }

    /// A shared server restarted and `init` re-made the steward's connection at it
    /// (servers/steward.md, "Failure and restart"): the kernel's records there are replaced, and
    /// every live session with a slot at that server is told, one `server_restarted` send per
    /// slot on its notices endpoint, so it re-binds ([`Steward::rebind`]). No event of the
    /// core's: the server grants nothing here it did not grant at the login. A lease's VM has no
    /// notices endpoint and is told nothing: its connections there stay dead.
    pub fn reconnected<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        server: &str,
        connection: Handle,
    ) {
        kernel.reconnected(server, connection);
        kernel.forget(server);
        let sessions: Vec<(Object, H)> = self.notices.iter().map(|(o, n)| (o.clone(), *n)).collect();
        for (object, n) in sessions {
            for slot in kernel.slots_at(&object, server) {
                kernel.restarted(n, server, slot);
            }
        }
    }

    /// The object routed by `badge` re-makes its connection at `slot`, in the scope its batch
    /// made: the one the batch made there died with its server's old instance. The new one takes
    /// the dead one's token, so the object's end releases it as it would have (R41's order), and
    /// the dead one's record is dropped. Answers the handle the reply carries and the path the
    /// child binds it at; `None` for a badge that routes nothing or a slot bound to nothing at a
    /// path.
    pub fn rebind<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        badge: u64,
        slot: u16,
    ) -> Result<Option<(Handle, String)>, Error> {
        let Some(route) = inspect::index(&self.store).routes.get(&badge).cloned() else { return Ok(None) };
        // A lease is not re-bound (servers/steward.md, "Failure and restart"; userland/agents.md):
        // its VM is told of no restart, and its sponsor's way out is `end_lease` and a new
        // `start_agent`; an agent's `rebind` is answered as a foreign slot's is.
        if route.kind == Kind::Lease {
            return Ok(None);
        }
        let owner = Object { domain: route.domain, kind: route.kind, id: route.id };
        let scope = self.budget(&Token { owner: owner.clone(), slot: SCOPE })?;
        let Some((key, handle, at)) =
            self.minting(kernel, &owner, Some(slot), |k| k.rebind(&owner, slot, scope))?
        else {
            return Ok(None);
        };
        let token =
            Token { owner, slot: CONNECTIONS + 1 + u8::try_from(slot).map_err(|_| Error::InvalidArgument)? };
        if let Some(Made::Connection(Some(old))) = self.made.insert(token, Made::Connection(Some(key))) {
            self.give_back(kernel, old);
        }
        Ok(Some((handle, at)))
    }

    /// Runs `batch`'s steps in order, stopping at the first that fails or at a launch the
    /// thread runs: every batch an event names runs here, and a host test runs a batch of its
    /// own.
    pub fn run<K: Kernel<Budget = B, Handle = H>>(&mut self, kernel: &mut K, batch: Batch) -> Ran {
        self.run_from(kernel, batch, 0, Vec::new())
    }

    /// [`Steward::run`] from step `from`, with `produced` what the steps before it made.
    fn run_from<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        batch: Batch,
        from: usize,
        mut produced: Vec<Produced>,
    ) -> Ran {
        for step in from..batch.steps.len() {
            let s = batch.steps[step].clone();
            match self.step(kernel, &batch.owner, &s) {
                Ok(Stepped::Made(made)) => produced.push(made),
                Ok(Stepped::Launching(job)) => {
                    return Ran::Pending(Pending { batch, step, produced, job });
                }
                Err(e) => {
                    self.failed.push((s, e));
                    return Ran::Done(Err(StepFailed { step, error: e as u32 }));
                }
            }
        }
        Ran::Done(Ok(produced))
    }

    /// Keeps a batch stopped at its launch, and the token of the call its `Done` will answer
    /// (the core's, stored with the object; none for a granted lease, and the console's for the
    /// console session, which the server never parks).
    fn suspend(&mut self, pending: Pending) {
        let owner = &pending.batch.owner;
        let reply = inspect::domain(&self.store, &owner.domain).and_then(|d| match owner.kind {
            Kind::Session => d.sessions.get(&owner.id).map(|s| s.reply),
            Kind::Lease => d.leases.get(&owner.id).map(|l| l.reply),
            _ => None,
        });
        if let Some(reply) = reply.filter(|r| *r != 0) {
            self.awaiting.insert(reply);
        }
        self.pending.push(pending);
    }

    /// Whether `kind` is an event of `object` that the core takes only once `object`'s batch
    /// is done: its process's exit, the end of it as a lease, its channel's close, and `sshd`'s
    /// end while it is a session (which detaches every attached context, the starting one's
    /// attachment among them). The tables mark each `unreachable` in `Starting`, since the
    /// embedder once ran every launch inside the event; a blame's `LockedOut`, the idle timer
    /// and a login taking a starting context over have rows there and are served.
    fn names(kind: &EventKind, object: &Object) -> bool {
        match kind {
            EventKind::Exited { object: o } => o == object,
            EventKind::EndLease { lease, .. } => *lease == object.id,
            EventKind::ChannelClosed { session } => object.kind == Kind::Session && *session == object.id,
            EventKind::SshdGone => object.kind == Kind::Session,
            _ => false,
        }
    }

    /// Whether `kind` names an object whose batch waits at its launch: held until the launch
    /// thread reports, in arrival order, as today's order had it wait behind the launch. Every
    /// other object's events are served meanwhile: a deadline's exit and its notice, a timer's
    /// wake, another lease's `end_lease`, answered ahead and at once as the protocol says.
    fn holds(&self, kind: &EventKind) -> bool {
        self.pending.iter().any(|p| Self::names(kind, &p.batch.owner))
    }

    /// The launch thread reported job `job` (servers/steward.md, "Leases"): the loop finishes
    /// the step the batch stopped at, keeping the process and a session's notices send right as
    /// a launch that started inline is kept, runs the batch's remaining steps, and only then
    /// hands the batch's outcome to the core as its `Done`, so the core sees the events of the
    /// object in the order it always did; a launch the thread failed fails the step as any
    /// step's failure does, and the core rolls the batch back. The events held for the object
    /// are then replayed in the order they came. Returns everything to deliver, replies to
    /// parked calls among them.
    pub fn launched<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        job: u64,
    ) -> Result<Vec<Output>, Exited> {
        let finished = kernel.launched(job);
        let Some(i) = self.pending.iter().position(|p| p.job == job) else {
            // No batch waits for it, which the tables exclude; what a child started holds
            // ends with the budget its forget destroyed, and the steward keeps nothing.
            if let Ok((_, Some(n))) = finished {
                self.give_back(kernel, n);
            }
            return Ok(Vec::new());
        };
        let Pending { batch, step, mut produced, .. } = self.pending.remove(i);
        let owner = batch.owner.clone();
        let ran = match finished {
            Ok((pid, notices)) => {
                self.started(kernel, &owner, &batch.steps[step], pid, notices);
                produced.push(Produced::Process(pid));
                self.run_from(kernel, batch, step + 1, produced)
            }
            Err(e) => {
                self.failed.push((batch.steps[step].clone(), e));
                Ran::Done(Err(StepFailed { step, error: e as u32 }))
            }
        };
        let mut outputs = match ran {
            Ran::Done(result) => self.event(kernel, EventKind::Done { object: owner.clone(), result }, 0)?,
            Ran::Pending(pending) => {
                self.suspend(pending);
                return Ok(Vec::new());
            }
        };
        let (mine, rest): (Vec<Held>, Vec<Held>) =
            core::mem::take(&mut self.held).into_iter().partition(|(kind, _)| Self::names(kind, &owner));
        self.held = rest;
        for (kind, reply) in mine {
            outputs.extend(self.event(kernel, kind, reply)?);
        }
        Ok(outputs)
    }

    /// `owner`'s child started, as `step` asked: its process is kept under the step's token and
    /// among the running ones; a session's notices send right under the session, an older one
    /// given back.
    fn started<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        owner: &Object,
        step: &Step,
        pid: u64,
        notices: Option<H>,
    ) {
        let Step::Launch { token, .. } = step else { return };
        self.made.insert(token.clone(), Made::Process(pid));
        self.pids.insert(pid, owner.clone());
        if let Some(n) = notices {
            if let Some(old) = self.notices.insert(owner.clone(), n) {
                self.give_back(kernel, old);
            }
        }
    }

    fn budget(&self, token: &Token) -> Result<B, Error> {
        match self.made.get(token) {
            Some(Made::Budget(b) | Made::Scope(b, _)) => Ok(*b),
            _ => Err(Error::BadHandle),
        }
    }

    /// Destroys budget `b` by the steward's handle, which goes with it ([`Kernel::destroy`]).
    /// `Dead` is the same end: the kernel took the budget already, at its deadline or with the
    /// budget above it, and the handle has been a dead entry since, which no later handle can
    /// take (I17); the destroy closes it all the same.
    fn destroy<K: Kernel<Budget = B, Handle = H>>(kernel: &mut K, b: B) -> Result<(), Error> {
        match kernel.destroy(b) {
            Err(Error::Dead) => Ok(()),
            destroyed => destroyed,
        }
    }

    /// Closes the scopes made in the budget `budget` names, which ended with it: each handle is a
    /// dead entry, and [`Steward::destroy`] closes it.
    fn end_scopes<K: Kernel<Budget = B, Handle = H>>(&mut self, kernel: &mut K, budget: &Token) {
        let ended: Vec<Token> = self
            .made
            .iter()
            .filter(|(_, m)| matches!(m, Made::Scope(_, parent) if parent == budget))
            .map(|(t, _)| t.clone())
            .collect();
        for t in ended {
            if let Some(Made::Scope(s, _)) = self.made.remove(&t) {
                let _ = Self::destroy(kernel, s);
            }
        }
    }

    fn step<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        owner: &Object,
        step: &Step,
    ) -> Result<Stepped, Error> {
        let made = match step {
            // A budget under `users` (a crossing's) is stamped with its domain's account, as
            // boot stamps a principal's top budget: it bills to the owner (R8), its crash is
            // blamed on the domain (R40), and its connection falls in the account's bucket at
            // its server (serving.md, "Buckets"). Every other budget inherits its parent's.
            Step::CreateBudget { token, parent, limits, labels, deadline } => {
                let (parent, account) = match parent {
                    Parent::Sub(domain) => (self.sub(domain).ok_or(Error::BadHandle)?, 0),
                    Parent::Budget(t) => (self.budget(t)?, 0),
                    Parent::Users => (self.users, owner.domain.account().get()),
                };
                let spec = spec(limits, labels.as_slice(), account, deadline.unwrap_or(FOREVER))?;
                let b = kernel.create(parent, &spec)?;
                self.made.insert(token.clone(), Made::Budget(b));
                Ok(Produced::Budget(kernel.budget_id(b)))
            }
            // A zero-limit budget: it holds nothing and runs nothing, so a server given it can
            // revoke what it stamped, never destroy the session (R41). It carries the session's
            // labels, as every budget under a labelled one must.
            Step::CreateScope { scope, budget } => {
                let parent = self.budget(budget)?;
                let zero = Limits::default();
                let s = kernel.create(parent, &spec(&zero, owner.domain.labels().as_slice(), 0, FOREVER)?)?;
                self.made.insert(scope.token().clone(), Made::Scope(s, budget.clone()));
                Ok(Produced::Scope)
            }
            // Every connection is minted in the object's scope, so the scope's end ends it in
            // every table at once (R41): the steward's own badge by its mint, a shared server's
            // by `new_connection`'s scope.
            Step::Connect { token, scope, server, badge } => {
                let scope = self.budget(scope.token())?;
                let h = match server {
                    // The session's requests come back on the steward's own endpoint, routed by
                    // this badge.
                    Server::Steward => Some(kernel.mint(*badge, scope)?),
                    Server::Shared(slot) => {
                        self.minting(kernel, owner, Some(*slot), |k| k.connect(owner, *slot, scope))?
                    }
                };
                self.made.insert(token.clone(), Made::Connection(h));
                Ok(Produced::Connection)
            }
            Step::Launch { token: _, budget, connections } => {
                let b = self.budget(budget)?;
                let mut held = Vec::new();
                for c in connections {
                    match self.made.get(c) {
                        Some(Made::Connection(h)) => held.push(*h),
                        _ => return Err(Error::BadHandle),
                    }
                }
                let session = self.store.domain(&owner.domain).and_then(|d| d.sessions.get(&owner.id));
                let context =
                    session.filter(|_| owner.kind == Kind::Session).and_then(|s| s.context.as_deref());
                // The launch thread runs the launch: the batch stops here, and the step is
                // finished when it reports ([`Steward::launched`]).
                match kernel.launch(owner, b, &held, context)? {
                    Launched::Pending(job) => return Ok(Stepped::Launching(job)),
                    Launched::Started(pid, notices) => {
                        self.started(kernel, owner, step, pid, notices);
                        Ok(Produced::Process(pid))
                    }
                }
            }
            Step::LaunchRelay { token, budget } => {
                let b = self.budget(budget)?;
                let (pid, control) = kernel.launch_relay(&owner.domain, b)?;
                self.made.insert(token.clone(), Made::Relay(pid, control));
                self.pids.insert(pid, owner.clone());
                Ok(Produced::Process(pid))
            }
            Step::Attach { relay, console, note } => {
                let Some(Made::Relay(_, control)) = self.made.get(relay) else {
                    return Err(Error::BadHandle);
                };
                let kept = kernel.attach(*control, note)?;
                self.made.insert(console.clone(), Made::Connection(Some(kept)));
                Ok(Produced::Done)
            }
            // The console is given back whatever the relay did: that is what tells `sshd` the
            // channel's session is over, and it never waits on code in the context's budget. A
            // detach past its bound is a detach: the relay lets the channel go as it takes the
            // call, and only the note, best effort, was still to be written to a channel that has
            // stopped reading (servers/consrelay.md, "The `consrelay` protocol"). A relay whose
            // process a `DestroyBudget` before this step took (a lockout's end batch, a dead VM's)
            // is gone with its budget: there is nobody to tell, and a call would only wait out its
            // bound while the steward serves nobody (servers/steward.md, "Failure and restart").
            // The console's `ended` to `sshd` is cleanup, the cleanup thread's.
            Step::Detach { relay, console, note } => {
                let told = match self.made.get(relay) {
                    Some(Made::Relay(pid, _)) if !self.pids.contains_key(pid) => Ok(()),
                    Some(Made::Relay(_, control)) => kernel.detach(*control, note),
                    _ => Err(Error::BadHandle),
                };
                if let Some(Made::Connection(Some(h))) = self.made.remove(console) {
                    self.give_back(kernel, h);
                }
                match told {
                    Ok(()) | Err(Error::Timeout) => Ok(Produced::Done),
                    Err(e) => Err(e),
                }
            }
            // The owner's process is ending: its exit notice is late from here on. A budget the
            // kernel's deadline took (a lease's at its `Exited`, or at an `end_lease` served
            // after the deadline but before the notice) is destroyed already, and its handle is
            // dead: the end the core asked for is done ([`Steward::destroy`]). The scopes made
            // in it ended with it.
            Step::DestroyBudget { budget } => {
                let b = self.budget(budget)?;
                self.end_pids(owner);
                Self::destroy(kernel, b)?;
                self.made.remove(budget);
                self.end_scopes(kernel, budget);
                Ok(Produced::Done)
            }
            // A crossing's read (servers/steward.md, "Declassification and push"; R42): the
            // item read through the reader budget, by the `crossing` program, which the kernel
            // calls once; the steward, unlabelled, never reads the vault (R1). The connection the
            // program held is kept under the crossing's token, given back once the budget is
            // destroyed. The steward's own read is a push's source, in the submitter's home,
            // bounded by `PUSH_MAX`.
            Step::Read { token: _, through, labels: _, item } => {
                let path = self.items.get(item).map(|i| i.path.clone()).ok_or(Error::BadHandle)?;
                match through {
                    Some(budget) => {
                        let budget = self.budget(budget)?;
                        let (pid, key, bytes) =
                            self.minting(kernel, owner, None, |k| k.cross(owner, budget, &path, None))?;
                        self.crossed(owner, pid, key);
                        Ok(Produced::Bytes(bytes))
                    }
                    None => {
                        let principal = self.principal_of(owner)?;
                        Ok(Produced::Bytes(kernel.home_read(&principal, &path, PUSH_MAX)?))
                    }
                }
            }
            // A crossing's write: a push's item written through the writer budget by the same
            // program; the steward's own write is a declassification's copy out, exactly the
            // snapshot (`Bytes::Literal`, never a re-read: R42) to `~/out/<name>` in the
            // submitter's home, refused over a file that is there.
            Step::Write { through, labels: _, item, bytes } => {
                let Bytes::Literal(bytes) = bytes else { return Err(Error::InvalidArgument) };
                let path = self.items.get(item).map(|i| i.path.clone()).ok_or(Error::BadHandle)?;
                match through {
                    Some(budget) => {
                        let budget = self.budget(budget)?;
                        let (pid, key, _) = self
                            .minting(kernel, owner, None, |k| k.cross(owner, budget, &path, Some(bytes)))?;
                        self.crossed(owner, pid, key);
                    }
                    None => {
                        let principal = self.principal_of(owner)?;
                        let name = split(&path).1;
                        kernel.home_write(&principal, &format!("{OUT}/{name}"), bytes)?;
                    }
                }
                Ok(Produced::Done)
            }
        };
        made.map(Stepped::Made)
    }

    /// The crossing program ran for `owner`: its process is `owner`'s, for the exit line, and its
    /// connection is held under `owner`'s token until the crossing is forgotten.
    fn crossed(&mut self, owner: &Object, pid: u64, key: Option<H>) {
        self.pids.insert(pid, owner.clone());
        if key.is_some() {
            let token = Token { owner: owner.clone(), slot: CROSSING_CONNECTION };
            self.made.insert(token, Made::Connection(key));
        }
    }

    /// The principal whose home `owner`'s domain's account names: the submitter's, for a
    /// request's or a crossing's steps there.
    fn principal_of(&self, owner: &Object) -> Result<String, Error> {
        self.principal_name(owner.domain.account().get()).ok_or(Error::BadHandle)
    }

    /// Drops what `object`'s steps made, in R41's order: a budget left is destroyed first, its
    /// scope and every connection minted in it ending with it (the cutoff, the kernel's); then
    /// each connection is given back ([`Steward::give_back`]): its server is told at the loop's
    /// turn and frees what it held for it (the cleanup, which is never the cutoff).
    fn forget<K: Kernel<Budget = B, Handle = H>>(&mut self, kernel: &mut K, object: &Object) {
        self.end_pids(object);
        if let Some(n) = self.notices.remove(object) {
            self.give_back(kernel, n);
        }
        let mine: Vec<Token> = self.made.keys().filter(|t| &t.owner == object).cloned().collect();
        let mut connections = Vec::new();
        let mut scopes = Vec::new();
        for t in mine {
            match self.made.remove(&t) {
                Some(Made::Connection(Some(h)) | Made::Relay(_, h)) => connections.push(h),
                Some(Made::Budget(b)) => {
                    let _ = Self::destroy(kernel, b);
                }
                Some(Made::Scope(s, _)) => scopes.push(s),
                _ => {}
            }
        }
        // Each made in the object's own budget, so ended with it: a dead entry, which the
        // destroy closes.
        for s in scopes {
            let _ = Self::destroy(kernel, s);
        }
        for h in connections {
            self.give_back(kernel, h);
        }
    }
}

/// An object that is no lease, as its exit line names it: a crossing by its id alone, anything
/// else by its kind and id.
pub fn named(object: &Object) -> String {
    match object.kind {
        Kind::Crossing => alloc::format!("{}", object.id),
        kind => alloc::format!("{kind:?} {}", object.id),
    }
}

/// An item's path as its directory and its name: `("", "note")` for an item at the root,
/// `("a/b", "note")` below it.
pub fn split(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", path),
    }
}

/// A budget's spec: `limits`, `labels`, `account` (0 for none of its own: it inherits its
/// parent's, R8), ending at `deadline`.
pub(crate) fn spec(l: &Limits, labels: &[u64], account: u64, deadline: u64) -> Result<BudgetSpec, Error> {
    Ok(BudgetSpec {
        pages: l.pages,
        processes: u32::try_from(l.processes).map_err(|_| Error::InvalidArgument)?,
        weight: u32::try_from(l.weight).map_err(|_| Error::InvalidArgument)?,
        labels: Labels::from_slice(labels)?,
        account,
        deadline,
    })
}

/// The steps' map: what each token names.
pub type MadeMap<B, H> = BTreeMap<Token, Made<B, H>>;
