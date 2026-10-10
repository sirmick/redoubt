//! The dispatch: routes each event to the machine of the object it is about, in that object's one
//! domain, and applies what the generated dispatch says (servers/steward.md, "Machines").
//!
//! Routing is outside the tables: the caller's role (by the event's kind), its badge through the
//! routing index, and the domain a principal and label set name. An event that names nothing is
//! refused with the same answer whatever it names (`Unknown`), or, raised by the core, dropped.

use alloc::string::String;
use alloc::vec::Vec;

use crate::cx::{Cx, Next, Out, Raised};
use crate::domain::{Domain, Labels};
use crate::effect::{Answer, Batch, Kind, Object, Output, Produced, Refusal, ReplyTo, StepFailed};
use crate::event::{Content, Event, EventKind};
use crate::gen::{self, Policy};
use crate::store::{Channel, DomainState, Lease, Parts, Request, Requester, Route, Session, Store};

/// One machine's state in a domain.
pub(crate) trait Machine {
    type State: Copy;
    type Event: Copy;
    const KIND: Kind;
    fn get(st: &DomainState, id: u64) -> Option<(Self::State, ReplyTo)>;
    fn set(st: &mut DomainState, id: u64, s: Self::State);
    fn remove(st: &mut DomainState, id: u64);
    fn is_final(s: Self::State) -> bool;
    fn dispatch(p: &Policy, cx: &mut Cx<'_>, from: Option<Self::State>, e: Self::Event) -> Next<Self::State>;
}

pub(crate) struct SessionM;
pub(crate) struct LeaseM;
pub(crate) struct RequestM;
pub(crate) struct CrossingM;
pub(crate) struct BlameM;

impl Machine for SessionM {
    type Event = gen::session::Event;
    type State = gen::session::State;

    const KIND: Kind = Kind::Session;

    fn get(st: &DomainState, id: u64) -> Option<(Self::State, ReplyTo)> {
        st.sessions.get(&id).map(|s| (s.state, s.reply))
    }

    fn set(st: &mut DomainState, id: u64, s: Self::State) {
        if let Some(o) = st.sessions.get_mut(&id) {
            o.state = s;
        }
    }

    fn remove(st: &mut DomainState, id: u64) { st.sessions.remove(&id); }

    fn is_final(s: Self::State) -> bool { s.is_final() }

    fn dispatch(p: &Policy, cx: &mut Cx<'_>, from: Option<Self::State>, e: Self::Event) -> Next<Self::State> {
        gen::session::dispatch(p, cx, from, e)
    }
}

impl Machine for LeaseM {
    type Event = gen::lease::Event;
    type State = gen::lease::State;

    const KIND: Kind = Kind::Lease;

    fn get(st: &DomainState, id: u64) -> Option<(Self::State, ReplyTo)> {
        st.leases.get(&id).map(|s| (s.state, s.reply))
    }

    fn set(st: &mut DomainState, id: u64, s: Self::State) {
        if let Some(o) = st.leases.get_mut(&id) {
            o.state = s;
        }
    }

    fn remove(st: &mut DomainState, id: u64) { st.leases.remove(&id); }

    fn is_final(s: Self::State) -> bool { s.is_final() }

    fn dispatch(p: &Policy, cx: &mut Cx<'_>, from: Option<Self::State>, e: Self::Event) -> Next<Self::State> {
        gen::lease::dispatch(p, cx, from, e)
    }
}

impl Machine for RequestM {
    type Event = gen::request::Event;
    type State = gen::request::State;

    const KIND: Kind = Kind::Request;

    fn get(st: &DomainState, id: u64) -> Option<(Self::State, ReplyTo)> {
        st.requests.get(&id).map(|s| (s.state, s.reply))
    }

    fn set(st: &mut DomainState, id: u64, s: Self::State) {
        if let Some(o) = st.requests.get_mut(&id) {
            o.state = s;
        }
    }

    fn remove(st: &mut DomainState, id: u64) { st.requests.remove(&id); }

    fn is_final(s: Self::State) -> bool { s.is_final() }

    fn dispatch(p: &Policy, cx: &mut Cx<'_>, from: Option<Self::State>, e: Self::Event) -> Next<Self::State> {
        gen::request::dispatch(p, cx, from, e)
    }
}

impl Machine for CrossingM {
    type Event = gen::crossing::Event;
    type State = gen::crossing::State;

    const KIND: Kind = Kind::Crossing;

    fn get(st: &DomainState, id: u64) -> Option<(Self::State, ReplyTo)> {
        st.crossings.get(&id).map(|s| (s.state, 0))
    }

    fn set(st: &mut DomainState, id: u64, s: Self::State) {
        if let Some(o) = st.crossings.get_mut(&id) {
            o.state = s;
        }
    }

    fn remove(st: &mut DomainState, id: u64) { st.crossings.remove(&id); }

    fn is_final(s: Self::State) -> bool { s.is_final() }

    fn dispatch(p: &Policy, cx: &mut Cx<'_>, from: Option<Self::State>, e: Self::Event) -> Next<Self::State> {
        gen::crossing::dispatch(p, cx, from, e)
    }
}

impl Machine for BlameM {
    type Event = gen::blame::Event;
    type State = gen::blame::State;

    const KIND: Kind = Kind::Blame;

    fn get(st: &DomainState, _: u64) -> Option<(Self::State, ReplyTo)> { Some((st.blame.state, 0)) }

    fn set(st: &mut DomainState, _: u64, s: Self::State) { st.blame.state = s; }

    fn remove(_: &mut DomainState, _: u64) {}

    fn is_final(s: Self::State) -> bool { s.is_final() }

    fn dispatch(p: &Policy, cx: &mut Cx<'_>, from: Option<Self::State>, e: Self::Event) -> Next<Self::State> {
        gen::blame::dispatch(p, cx, from, e)
    }
}

/// What an event brings to one transition besides its object.
#[derive(Clone)]
pub(crate) struct Call<'e> {
    pub event: &'e Event,
    pub caller: Option<Route>,
    pub channel: Option<u64>,
    pub result: Option<&'e Result<Vec<Produced>, StepFailed>>,
    /// Whether the object is new: created by this transition, or removed again.
    pub created: bool,
}

impl<'e> Call<'e> {
    pub(crate) fn of(event: &'e Event) -> Call<'e> {
        Call { event, caller: None, channel: None, result: None, created: false }
    }
}

/// Runs one transition of machine `M` for object `id` of `domain`, with `grant` the second
/// domain only the request and approval path reads. A new object is already in the state.
pub(crate) fn run_in<M: Machine>(
    p: &Policy,
    store_parts: Parts<'_>,
    grant: Option<&DomainState>,
    call: &Call<'_>,
    out: &mut Out,
    id: u64,
    e: M::Event,
) -> Next<M::State> {
    let Parts { fixed, index, used, domain, state } = store_parts;
    let (from, stored) = match M::get(state, id) {
        Some((s, r)) => (if call.created { None } else { Some(s) }, r),
        None => (None, 0),
    };
    // A batch's outcome answers the call that started the batch.
    let reply = if call.result.is_some() { stored } else { call.event.reply };
    let mut cx = Cx {
        fixed,
        index,
        used,
        domain,
        state,
        grant,
        event: call.event,
        caller: call.caller.clone(),
        channel: call.channel,
        kind: M::KIND,
        id,
        result: call.result,
        reply,
        reason: None,
        out,
    };
    let next = M::dispatch(p, &mut cx, from, e);
    let object = Object { domain: domain.clone(), kind: M::KIND, id };
    match next {
        Next::To(s) if M::is_final(s) => {
            M::remove(cx.state, id);
            cx.index.ids.remove(&id);
            cx.index.attachments.retain(|_, (_, s)| *s != id);
        }
        Next::To(s) => M::set(cx.state, id, s),
        Next::Stay => {}
        Next::Nothing | Next::NoRow => {
            if call.created {
                M::remove(cx.state, id);
                cx.index.ids.remove(&id);
            }
        }
        Next::Unreachable => cx.out.effects.exit = true,
    }
    let steps = core::mem::take(&mut out.steps);
    if !steps.is_empty() {
        out.effects.batches.push(Batch { owner: object, steps });
    }
    next
}

/// Runs one transition in `domain`, which the store holds; `NoRow` if it holds no such domain.
pub(crate) fn run<M: Machine>(
    store: &mut Store,
    call: &Call<'_>,
    out: &mut Out,
    domain: &Domain,
    id: u64,
    e: M::Event,
) -> Next<M::State> {
    let Some((policy, parts)) = store.parts(domain) else { return Next::NoRow };
    run_in::<M>(policy, parts, None, call, out, id, e)
}

/// Answers an external call that named nothing.
pub(crate) fn unknown(event: &Event, out: &mut Out) {
    if event.reply != 0 {
        out.effects
            .outputs
            .push(Output::Reply { to: event.reply, answer: Answer::Refused(Refusal::Unknown) });
    }
}

fn refuse(event: &Event, out: &mut Out, why: Refusal) {
    if event.reply != 0 {
        out.effects.outputs.push(Output::Reply { to: event.reply, answer: Answer::Refused(why) });
    }
}

pub(crate) fn answered<S>(next: Next<S>, event: &Event, out: &mut Out) {
    if matches!(next, Next::NoRow) {
        unknown(event, out);
    }
}

/// A fresh id from the event's words, as an effect draws one.
pub(crate) fn fresh(store: &Store, event: &Event, out: &mut Out) -> u64 {
    for w in event.random {
        if store.index.fresh(w) && !store.used.contains_key(&w) && !out.drawn.contains(&w) {
            out.drawn.push(w);
            return w;
        }
    }
    out.effects.exit = true;
    0
}

/// Puts a new object in its domain (and its id in the index) before its first transition.
fn insert(store: &mut Store, domain: &Domain, kind: Kind, id: u64, f: impl FnOnce(&mut DomainState)) -> bool {
    let Some(state) = store.domain_mut(domain) else { return false };
    f(state);
    match kind {
        Kind::Session | Kind::Lease => {
            store.index.ids.insert(id, (domain.clone(), kind));
        }
        _ => {
            store.used.insert(id, domain.clone());
        }
    }
    true
}

/// A request or crossing id leaves `used` when its object goes.
pub(crate) fn release(store: &mut Store, domain: &Domain, kind: Kind, id: u64) {
    if !store.domain(domain).is_some_and(|d| d.has(kind, id)) {
        store.used.remove(&id);
    }
}

/// The route of a session's or lease's badge.
pub(crate) fn route(store: &Store, badge: u64) -> Option<Route> { store.index.routes.get(&badge).cloned() }

/// A session of principal `p` in `domain`, opened by a login with `key` as `context`, or as the
/// console principal's (key 0, no context): the domain's blame machine sees the login (a lockout
/// window that has passed ends; nothing is counted), the session is made, and its machine started
/// on `opened`.
#[allow(clippy::too_many_arguments)]
fn open_session(
    store: &mut Store,
    call: &Call<'_>,
    event: &Event,
    out: &mut Out,
    p: usize,
    domain: &Domain,
    key: u64,
    context: Option<String>,
    opened: gen::session::Event,
) {
    run::<BlameM>(store, call, out, domain, 0, gen::blame::Event::Login);
    let (id, badge) = (fresh(store, event, out), fresh(store, event, out));
    let s = Session {
        id,
        state: gen::session::State::Starting,
        principal: p,
        key,
        context,
        badge,
        number: 0,
        reply: event.reply,
        attachment: id,
        from: String::new(),
    };
    insert(store, domain, Kind::Session, id, |st| {
        st.sessions.insert(id, s);
    });
    let call = Call { created: true, ..call.clone() };
    run::<SessionM>(store, &call, out, domain, id, opened);
}

/// An event from outside the core.
pub(crate) fn external(store: &mut Store, event: &Event, out: &mut Out) {
    let call = Call::of(event);
    match &event.kind {
        EventKind::Login { principal, labels, context, key, .. } => {
            // Before the key is checked, every refusal is the bad key's: an unknown principal, a
            // label set the manifest does not give it and a context that is not a name are told
            // apart from a wrong key by nobody (servers/steward.md, "Contexts").
            let Some(p) = store.fixed.principal(principal) else {
                return refuse(event, out, Refusal::BadKey);
            };
            let account = store.fixed.principals[p].account.get();
            let Some(domain) = store.find(account, labels) else {
                return refuse(event, out, Refusal::BadKey);
            };
            if !context.is_empty() && !crate::manifest::name(context) {
                return refuse(event, out, Refusal::BadKey);
            }
            let context = Some(context.clone());
            open_session(store, &call, event, out, p, &domain, *key, context, gen::session::Event::Login);
        }
        EventKind::Console { principal } => {
            let Some(p) = store.fixed.principal(principal) else { return unknown(event, out) };
            let account = store.fixed.principals[p].account.get();
            let Some(domain) = store.find(account, &[]) else { return unknown(event, out) };
            open_session(store, &call, event, out, p, &domain, 0, None, gen::session::Event::Console);
        }
        EventKind::ChannelClosed { session } => {
            // A channel is named by its attachment; one no context holds now names nothing.
            let Some((domain, id)) = store.index.attachments.get(session).cloned() else {
                return unknown(event, out);
            };
            let next = run::<SessionM>(store, &call, out, &domain, id, gen::session::Event::ChannelClosed);
            answered(next, event, out);
        }
        EventKind::SshdGone => {
            // Every channel went with `sshd`: each attached context is detached.
            let attached: Vec<(Domain, u64)> = store.index.attachments.values().cloned().collect();
            for (domain, id) in attached {
                run::<SessionM>(store, &call, out, &domain, id, gen::session::Event::Detach);
            }
        }
        EventKind::ApprovalOpened { .. } | EventKind::ApprovalClosed { .. } => channel(store, event, out),
        EventKind::StartAgent { badge, lease } => {
            let Some(by) = route(store, *badge) else { return unknown(event, out) };
            let Some(principal) = store.fixed.by_account(by.domain.account()) else {
                return unknown(event, out);
            };
            let (id, own) = (fresh(store, event, out), fresh(store, event, out));
            let l = Lease {
                id,
                state: gen::lease::State::Starting,
                principal,
                badge: own,
                number: 0,
                lease: *lease,
                deadline: 0,
                parent: (by.kind == Kind::Lease).then_some(by.id),
                granted: false,
                reply: event.reply,
            };
            let domain = by.domain.clone();
            insert(store, &domain, Kind::Lease, id, |st| {
                st.leases.insert(id, l);
            });
            let call = Call { caller: Some(by), created: true, ..call };
            run::<LeaseM>(store, &call, out, &domain, id, gen::lease::Event::StartAgent);
        }
        EventKind::Submit { badge, content, reason } => {
            let Some(by) = route(store, *badge) else { return unknown(event, out) };
            let Some(principal) = store.fixed.by_account(by.domain.account()) else {
                return unknown(event, out);
            };
            // The domain its records are read under: the target's, which must be one of the
            // principal's (the same answer as `owns_labels`).
            let audit = match content {
                Content::Agent { labels, .. } | Content::Push { target: labels, .. } => {
                    match store.find(by.domain.account().get(), labels) {
                        Some(d) => d,
                        None => return refuse(event, out, Refusal::NotOwner),
                    }
                }
                _ => by.domain.clone(),
            };
            let id = fresh(store, event, out);
            let r = Request {
                id,
                state: gen::request::State::Frozen,
                by: Requester { kind: by.kind, id: by.id },
                principal,
                content: content.clone(),
                reason: reason.clone(),
                snapshot: None,
                reader: 0,
                hash: [0; 32],
                audit,
                channel: None,
                reply: event.reply,
            };
            let domain = by.domain.clone();
            insert(store, &domain, Kind::Request, id, |st| {
                st.requests.insert(id, r);
            });
            let call = Call { caller: Some(by), created: true, ..call };
            run::<RequestM>(store, &call, out, &domain, id, gen::request::Event::Submit);
            release(store, &domain, Kind::Request, id);
        }
        EventKind::EndLease { .. } => crate::edges::end_lease(store, event, out),
        EventKind::EndSession { badge } => {
            let Some(by) = route(store, *badge).filter(|r| r.kind == Kind::Session) else {
                return unknown(event, out);
            };
            let call = Call { caller: Some(by.clone()), ..call };
            run::<SessionM>(store, &call, out, &by.domain, by.id, gen::session::Event::EndSession);
        }
        EventKind::Pending { .. } | EventKind::Approve { .. } | EventKind::Deny { .. } => {
            crate::edges::approval(store, event, out)
        }
        EventKind::Blame { account, labels } => {
            // A crash with no current call blames nobody.
            if let Some(domain) = store.find(*account, labels) {
                run::<BlameM>(store, &call, out, &domain, 0, gen::blame::Event::Blame);
            }
        }
        EventKind::Exited { object } => {
            let d = &object.domain;
            match object.kind {
                Kind::Session => {
                    run::<SessionM>(store, &call, out, d, object.id, gen::session::Event::Exited);
                }
                Kind::Lease => {
                    run::<LeaseM>(store, &call, out, d, object.id, gen::lease::Event::Exited);
                }
                Kind::Crossing => {
                    run::<CrossingM>(store, &call, out, d, object.id, gen::crossing::Event::Exited);
                    release(store, d, Kind::Crossing, object.id);
                }
                _ => {}
            }
        }
        EventKind::Done { object, result } => {
            let call = Call { result: Some(result), ..call };
            let ok = result.is_ok();
            let d = &object.domain;
            match object.kind {
                Kind::Session => {
                    let e = if ok { gen::session::Event::Done } else { gen::session::Event::Failed };
                    run::<SessionM>(store, &call, out, d, object.id, e);
                }
                Kind::Lease => {
                    let e = if ok { gen::lease::Event::Done } else { gen::lease::Event::Failed };
                    run::<LeaseM>(store, &call, out, d, object.id, e);
                }
                Kind::Request => {
                    // A push's source, read by the steward: its snapshot.
                    if let Ok(list) = result {
                        let bytes = list.iter().find_map(|p| match p {
                            Produced::Bytes(b) => Some(b.clone()),
                            _ => None,
                        });
                        if let Some(r) = store.domain_mut(d).and_then(|s| s.requests.get_mut(&object.id)) {
                            r.snapshot = bytes;
                        }
                    }
                    let e = if ok { gen::request::Event::Done } else { gen::request::Event::Failed };
                    run::<RequestM>(store, &call, out, d, object.id, e);
                    release(store, d, Kind::Request, object.id);
                }
                Kind::Crossing => {
                    let e = if ok { gen::crossing::Event::Done } else { gen::crossing::Event::Failed };
                    run::<CrossingM>(store, &call, out, d, object.id, e);
                    release(store, d, Kind::Crossing, object.id);
                }
                Kind::Blame | Kind::Channel => {}
            }
        }
    }
}

/// An event one machine raised for another. One that names an object already gone is dropped.
pub(crate) fn internal(store: &mut Store, event: &Event, raised: Raised, out: &mut Out) {
    let call = Call::of(event);
    match raised {
        Raised::Granted { domain, id, principal, lease } => {
            let badge = fresh(store, event, out);
            let l = Lease {
                id,
                state: gen::lease::State::Starting,
                principal,
                badge,
                number: 0,
                lease,
                deadline: 0,
                parent: None,
                granted: true,
                reply: 0,
            };
            if insert(store, &domain, Kind::Lease, id, |st| {
                st.leases.insert(id, l);
            }) {
                let call = Call { created: true, ..call };
                run::<LeaseM>(store, &call, out, &domain, id, gen::lease::Event::Granted);
            }
        }
        Raised::Open { domain, crossing } => crate::edges::cross(store, event, domain, crossing, out),
        Raised::Snapshot { request, result } => {
            let d = &request.domain;
            let Some(r) = store.domain_mut(d).and_then(|s| s.requests.get_mut(&request.id)) else { return };
            let ok = result.is_some();
            if let Some((bytes, reader)) = result {
                r.snapshot = Some(bytes);
                r.reader = reader;
            }
            // The request's batch is the crossing's: its outcome answers the submission.
            let outcome: Result<Vec<Produced>, StepFailed> =
                if ok { Ok(Vec::new()) } else { Err(StepFailed { step: 0, error: 0 }) };
            let call = Call { result: Some(&outcome), ..call };
            let e = if ok { gen::request::Event::Done } else { gen::request::Event::Failed };
            run::<RequestM>(store, &call, out, d, request.id, e);
            release(store, d, Kind::Request, request.id);
        }
        Raised::LockedOut { object } => match object.kind {
            Kind::Session => {
                run::<SessionM>(store, &call, out, &object.domain, object.id, gen::session::Event::LockedOut);
            }
            Kind::Lease => {
                run::<LeaseM>(store, &call, out, &object.domain, object.id, gen::lease::Event::LockedOut);
            }
            _ => {}
        },
        Raised::Attach { object } => {
            run::<SessionM>(store, &call, out, &object.domain, object.id, gen::session::Event::Attach);
        }
        Raised::SessionEnded { request } => {
            run::<RequestM>(
                store,
                &call,
                out,
                &request.domain,
                request.id,
                gen::request::Event::SessionEnded,
            );
            release(store, &request.domain, Kind::Request, request.id);
        }
    }
}

/// An approval channel opening or closing.
pub(crate) fn channel(store: &mut Store, event: &Event, out: &mut Out) {
    match &event.kind {
        EventKind::ApprovalOpened { channel, principal, key } => {
            let Some(p) = store.fixed.principal(principal) else { return unknown(event, out) };
            if let Some(c) = store.index.channels.get(channel) {
                // The same channel opened twice: its own row refuses it.
                let p = c.principal;
                run_channel(
                    store,
                    &Call::of(event),
                    out,
                    *channel,
                    p,
                    gen::approval_channel::Event::ApprovalOpened,
                );
                return;
            }
            if !store.index.fresh(*channel) {
                return unknown(event, out);
            }
            new_channel(store, *channel, p, *key);
            let call = Call { created: true, ..Call::of(event) };
            run_channel(store, &call, out, *channel, p, gen::approval_channel::Event::ApprovalOpened);
        }
        EventKind::ApprovalClosed { channel } => {
            let Some(p) = store.index.channels.get(channel).map(|c| c.principal) else {
                return unknown(event, out);
            };
            run_channel(
                store,
                &Call::of(event),
                out,
                *channel,
                p,
                gen::approval_channel::Event::ApprovalClosed,
            );
        }
        _ => {}
    }
}

/// A new approval channel, or one closing: outside every domain. It runs against a scratch
/// domain of its principal, which it never reads.
fn run_channel(
    store: &mut Store,
    call: &Call<'_>,
    out: &mut Out,
    id: u64,
    principal: usize,
    e: gen::approval_channel::Event,
) -> Next<gen::approval_channel::State> {
    let account = store.fixed.principals[principal].account;
    let domain = Domain::new(account, Labels::empty());
    let mut scratch = DomainState::scratch();
    let from = if call.created { None } else { store.index.channels.get(&id).map(|c| c.state) };
    let Store { fixed, index, used, policy, .. } = store;
    let mut cx = Cx {
        fixed,
        index,
        used,
        domain: &domain,
        state: &mut scratch,
        grant: None,
        event: call.event,
        caller: None,
        channel: Some(id),
        kind: Kind::Channel,
        id,
        result: None,
        reply: call.event.reply,
        reason: None,
        out,
    };
    let next = gen::approval_channel::dispatch(policy, &mut cx, from, e);
    match next {
        Next::To(s) if s.is_final() => {
            store.index.channels.remove(&id);
            store.unbind(id);
        }
        Next::To(s) => {
            if let Some(c) = store.index.channels.get_mut(&id) {
                c.state = s;
            }
        }
        Next::Nothing | Next::NoRow if call.created => {
            store.index.channels.remove(&id);
        }
        Next::Unreachable => out.effects.exit = true,
        _ => {}
    }
    next
}

/// A new channel's provisional entry, before its first transition.
fn new_channel(store: &mut Store, id: u64, principal: usize, key: u64) {
    let c = Channel { id, state: gen::approval_channel::State::Open, principal, key };
    store.index.channels.insert(id, c);
}
