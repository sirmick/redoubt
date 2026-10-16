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
    Answer, Batch, Kind, Notice, Notified, Object, Output, Parent, Produced, Rendered, ReplyTo, Server, Step,
    StepFailed, Token,
};
use redoubt_steward::effects::{CONNECTIONS, SCOPE};
use redoubt_steward::event::{Event, EventKind};
use redoubt_steward::manifest::Limits;
use redoubt_steward::{Effects, decide, inspect};

use crate::{Kernel, Steward};

/// Every word the server hands the core has this bit set, so each id and badge the core draws
/// from them is in the minted range (servers/serving.md R27): a session's badge can never be a
/// root badge, whatever the generator gave. 63 random bits remain (R36).
pub const MINTED: u64 = 1 << 63;

/// The reply token of the console session's event: its answer names the session the steward
/// reopens when it ends. A protocol call's token is 1.
const CONSOLE: ReplyTo = 2;

/// What a step made, kept under its token until the core forgets its owner.
pub enum Made<B, H> {
    Budget(B),
    Scope(B),
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

impl<B: Copy, H: Copy> Steward<B, H> {
    /// Decides `kind`, with `reply` the caller's token (0 for none), runs every batch that follows
    /// and returns the outputs for the server to deliver: replies, notices, screens, audit
    /// records. What the core forgets is dropped here.
    pub fn event<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        kind: EventKind,
        reply: ReplyTo,
    ) -> Result<Vec<Output>, Exited> {
        let mut outputs = Vec::new();
        let mut reopen = false;
        // The approval channels this event redraws (a `Pending` of theirs ran), and the screens
        // the core rendered on each: written as one block per channel at the end.
        let mut redrawn = BTreeSet::new();
        let mut screens: BTreeMap<u64, Vec<Rendered>> = BTreeMap::new();
        let mut queue = alloc::collections::VecDeque::new();
        queue.push_back((kind, reply));
        while let Some((kind, reply)) = queue.pop_front() {
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
            let Effects { outputs: out, batches, exit } = decide(&mut self.store, event);
            if exit {
                return Err(Exited);
            }
            for o in out {
                match o {
                    Output::Forget(object) => {
                        reopen |= self.console_session == Some(object.id) && object.kind == Kind::Session;
                        self.forget(kernel, &object);
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
                    // The sponsor's notice goes now, while the lease is still in the store to be
                    // named; what it says is the lease's name alone.
                    Output::Notice { to: Notified::Session(badge), notice: Notice::LeaseEnded { lease } } => {
                        self.notify(kernel, badge, lease);
                    }
                    Output::Notice { to: Notified::Session(badge), notice: Notice::ApprovalWaiting } => {
                        if let Some(n) = self.notices_of(badge) {
                            kernel.notify_approval(n);
                        }
                    }
                    // An approval channel of the requester's principal is open: it is redrawn,
                    // which renders the request on it (servers/steward.md, "The powerbox and
                    // approvals").
                    Output::Notice { to: Notified::Channel(channel), notice: Notice::ApprovalWaiting } => {
                        queue.push_back((EventKind::Pending { channel }, 0));
                    }
                    Output::Screen { channel, screen } => screens.entry(channel).or_default().push(screen),
                    o => outputs.push(o),
                }
            }
            for batch in batches {
                let object = batch.owner.clone();
                let result = self.run(kernel, &batch);
                queue.push_back((EventKind::Done { object, result }, 0));
            }
        }
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

    /// The screen an approval channel shows (servers/steward.md, "The powerbox and approvals";
    /// sshd.md, "`approve@box`"): a header naming the channel's principal; per request its id,
    /// its binding hash and the hash's first four bytes as the code a person types in its
    /// place, then the core's rendered line (`render.rs`: printable ASCII only, so nothing here
    /// but these bytes and the line ends reaches the terminal); and the line to type. Written as
    /// `\r\n` lines: the channel has no line discipline.
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
        let name =
            self.lease_name(owner.id).unwrap_or_else(|| alloc::format!("{:?} {}", owner.kind, owner.id));
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
        let Some((key, handle, at)) = kernel.rebind(&owner, slot, scope)? else { return Ok(None) };
        let token =
            Token { owner, slot: CONNECTIONS + 1 + u8::try_from(slot).map_err(|_| Error::InvalidArgument)? };
        if let Some(Made::Connection(Some(old))) = self.made.insert(token, Made::Connection(Some(key))) {
            kernel.release(old);
        }
        Ok(Some((handle, at)))
    }

    /// Runs `batch`'s steps in order, stopping at the first that fails.
    fn run<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        batch: &Batch,
    ) -> Result<Vec<Produced>, StepFailed> {
        let mut produced = Vec::new();
        for (step, s) in batch.steps.iter().enumerate() {
            let made = self.step(kernel, &batch.owner, s).inspect_err(|e| self.failed.push((s.clone(), *e)));
            produced.push(made.map_err(|e| StepFailed { step, error: e as u32 })?);
        }
        Ok(produced)
    }

    fn budget(&self, token: &Token) -> Result<B, Error> {
        match self.made.get(token) {
            Some(Made::Budget(b) | Made::Scope(b)) => Ok(*b),
            _ => Err(Error::BadHandle),
        }
    }

    fn step<K: Kernel<Budget = B, Handle = H>>(
        &mut self,
        kernel: &mut K,
        owner: &Object,
        step: &Step,
    ) -> Result<Produced, Error> {
        match step {
            Step::CreateBudget { token, parent, limits, labels, deadline } => {
                let parent = match parent {
                    Parent::Sub(domain) => self.sub(domain).ok_or(Error::BadHandle)?,
                    Parent::Budget(t) => self.budget(t)?,
                    Parent::Users => self.users,
                };
                let spec = spec(limits, labels.as_slice(), 0, deadline.unwrap_or(FOREVER))?;
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
                self.made.insert(scope.token().clone(), Made::Scope(s));
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
                    Server::Shared(slot) => kernel.connect(owner, *slot, scope)?,
                };
                self.made.insert(token.clone(), Made::Connection(h));
                Ok(Produced::Connection)
            }
            Step::Launch { token, budget, connections } => {
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
                let (pid, notices) = kernel.launch(owner, b, &held, context)?;
                self.made.insert(token.clone(), Made::Process(pid));
                self.pids.insert(pid, owner.clone());
                if let Some(n) = notices {
                    if let Some(old) = self.notices.insert(owner.clone(), n) {
                        kernel.release(old);
                    }
                }
                Ok(Produced::Process(pid))
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
            Step::Detach { relay, console, note } => {
                let told = match self.made.get(relay) {
                    Some(Made::Relay(pid, _)) if !self.pids.contains_key(pid) => Ok(()),
                    Some(Made::Relay(_, control)) => kernel.detach(*control, note),
                    _ => Err(Error::BadHandle),
                };
                if let Some(Made::Connection(Some(h))) = self.made.remove(console) {
                    kernel.release(h);
                }
                match told {
                    Ok(()) | Err(Error::Timeout) => Ok(Produced::Done),
                    Err(e) => Err(e),
                }
            }
            // The owner's process is ending: its exit notice is late from here on. A lease the
            // kernel's deadline took is already gone, its handle revoked with it (R10), and its
            // `Exited` reaches here with no process left: that destroy's `BadHandle` is the end
            // the core asked for, not a failure. On a budget whose process still runs it is one.
            Step::DestroyBudget { budget } => {
                let b = self.budget(budget)?;
                let running = self.pids.values().any(|o| o == owner);
                self.end_pids(owner);
                match kernel.destroy(b) {
                    Ok(()) => {}
                    Err(Error::BadHandle) if !running => {}
                    Err(e) => return Err(e),
                }
                self.made.remove(budget);
                Ok(Produced::Done)
            }
            // Crossings are STEWARD4's: no step reads or writes a volume yet.
            Step::Read { .. } | Step::Write { .. } => Err(Error::InvalidArgument),
        }
    }

    /// Drops what `object`'s steps made, in R41's order: a budget left is destroyed first, its
    /// scope and every connection minted in it ending with it (the cutoff, the kernel's); then
    /// each connection is given back to its server, which frees what it held for it (the
    /// cleanup, which is never the cutoff).
    fn forget<K: Kernel<Budget = B, Handle = H>>(&mut self, kernel: &mut K, object: &Object) {
        self.end_pids(object);
        if let Some(n) = self.notices.remove(object) {
            kernel.release(n);
        }
        let mine: Vec<Token> = self.made.keys().filter(|t| &t.owner == object).cloned().collect();
        let mut connections = Vec::new();
        for t in mine {
            match self.made.remove(&t) {
                Some(Made::Connection(Some(h)) | Made::Relay(_, h)) => connections.push(h),
                Some(Made::Budget(b)) => {
                    let _ = kernel.destroy(b);
                }
                _ => {}
            }
        }
        for h in connections {
            kernel.release(h);
        }
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
