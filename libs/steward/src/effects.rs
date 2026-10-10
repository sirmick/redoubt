//! The effects the tables name (servers/steward.md, "Guards and effects"). Each runs in one
//! transition's context: it changes that domain's object, emits steps to the object's batch, or
//! emits outputs and raised events. None branches on an object's kind behind its name.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::audit::Record;
use crate::consts::{BLAME_COUNT, BLAME_WINDOW, CROSSING_LIFE};
use crate::cx::{Cx, Raised};
use crate::domain::Labels;
use crate::effect::{
    Answer, Bytes, Kind, Notice, Notified, Object, Output, Parent, Refusal, Scope, Server, Step, Token,
};
use crate::event::{Content, EventKind};
use crate::guards::{lease, request, session};
use crate::store::{Crossing, CrossingKind, Route};

/// The slots of what a session's or a lease's batch makes.
const BUDGET: u8 = 0;
const SCOPE: u8 = 1;
const PROCESS: u8 = 2;
/// The connection to the steward; the shared servers' follow, one each.
const CONNECTIONS: u8 = 3;
/// A context's console relay, and the channel console it holds while attached.
const RELAY: u8 = 200;
const CONSOLE: u8 = 201;
/// What a read makes, in a crossing's or a request's batch.
const READ: u8 = 1;

/// Answers the caller with the row's refusal: the failed guard's reason, or the failed step.
pub fn refuse(cx: &mut Cx<'_>) {
    let failed = matches!(cx.result, Some(Err(_))).then_some(Refusal::Failed);
    let why = cx.reason.or(failed).unwrap_or(Refusal::Unknown);
    cx.reply(Answer::Refused(why));
}

pub fn reply_ok(cx: &mut Cx<'_>) { cx.reply(Answer::Ok); }

/// The object is gone: the embedder drops its tokens.
pub fn forget(cx: &mut Cx<'_>) {
    let o = cx.object();
    cx.output(Output::Forget(o));
}

// Sessions and leases.

/// A session's budget, from its domain's fixed sub-budget.
pub fn carve_session(cx: &mut Cx<'_>) {
    let step = Step::CreateBudget {
        token: cx.token(BUDGET),
        parent: Parent::Sub(cx.domain.clone()),
        limits: cx.fixed.sizes.session,
        labels: cx.domain.labels().clone(),
        deadline: None,
    };
    cx.step(step);
}

/// R39: a lease from its domain's sub-budget; a sub-agent inside its agent's budget, ending no
/// later.
pub fn carve_lease(cx: &mut Cx<'_>) {
    let now = cx.now();
    let Some(l) = lease(cx) else { return };
    let mut deadline = now.saturating_add(l.lease);
    let (parent, limits) = match l.parent.and_then(|a| cx.state.leases.get(&a)) {
        Some(agent) => {
            deadline = deadline.min(agent.deadline);
            let owner = Object { domain: cx.domain.clone(), kind: Kind::Lease, id: agent.id };
            (Parent::Budget(Token { owner, slot: BUDGET }), cx.fixed.sizes.sub_agent)
        }
        None => (Parent::Sub(cx.domain.clone()), cx.fixed.sizes.agent),
    };
    carve_lease_in(cx, parent, limits, deadline);
}

/// A lease's budget from `parent`, with `limits`, ending at `deadline`.
pub fn carve_lease_in(cx: &mut Cx<'_>, parent: Parent, limits: crate::manifest::Limits, deadline: u64) {
    if let Some(l) = cx.state.leases.get_mut(&cx.id) {
        l.deadline = deadline;
    }
    let step = Step::CreateBudget {
        token: cx.token(BUDGET),
        parent,
        limits,
        labels: cx.domain.labels().clone(),
        deadline: Some(deadline),
    };
    cx.step(step);
}

/// A revocation scope inside the new budget, for the namespace's connections (R41).
pub fn create_scope(cx: &mut Cx<'_>) {
    let step = Step::scope(cx.token(SCOPE), cx.token(BUDGET));
    cx.step(step);
}

/// The object's badge on the steward's endpoint.
fn badge(cx: &Cx<'_>) -> u64 {
    match cx.kind {
        Kind::Session => session(cx).map_or(0, |s| s.badge),
        Kind::Lease => lease(cx).map_or(0, |l| l.badge),
        _ => 0,
    }
}

/// Fresh connections, each narrowed to the scope: one to the steward, routed by the object's
/// badge, and one to each shared server, with a fresh badge.
pub fn connect(cx: &mut Cx<'_>) {
    // The scope this batch made: a connection takes nothing else.
    let Some(scope) = cx.out.steps.iter().rev().find_map(|s| match s {
        Step::CreateScope { scope, .. } => Some(scope.clone()),
        _ => None,
    }) else {
        return;
    };
    let own = badge(cx);
    connect_one(cx, CONNECTIONS, &scope, Server::Steward, own);
    for i in 0..cx.fixed.servers {
        let b = cx.fresh();
        connect_one(cx, CONNECTIONS + 1 + i as u8, &scope, Server::Shared(i), b);
    }
}

fn connect_one(cx: &mut Cx<'_>, slot: u8, scope: &Scope, server: Server, badge: u64) {
    let step = Step::Connect { token: cx.token(slot), scope: scope.clone(), server, badge };
    cx.step(step);
}

/// The process, through the loader stub, with the namespace.
pub fn launch(cx: &mut Cx<'_>) {
    let connections: Vec<Token> = (0..=cx.fixed.servers).map(|i| cx.token(CONNECTIONS + i as u8)).collect();
    let step = Step::Launch { token: cx.token(PROCESS), budget: cx.token(BUDGET), connections };
    cx.step(step);
}

/// A context's console relay, in the session's budget, before the process whose `/dev/cons` it
/// serves (servers/steward.md, "Contexts"). The console's session is no context and has none.
pub fn launch_relay(cx: &mut Cx<'_>) {
    if session(cx).is_some_and(|s| s.context.is_some()) {
        let step = Step::LaunchRelay { token: cx.token(RELAY), budget: cx.token(BUDGET) };
        cx.step(step);
    }
}

/// The login's address, as a channel note shows it: `a.b.c.d:port` as `sshd` gave it, or
/// `an unknown address` for anything else, so no byte of it can drive a terminal.
fn login_from(cx: &Cx<'_>) -> String {
    let EventKind::Login { from, .. } = &cx.event.kind else { return String::new() };
    let ok = !from.is_empty()
        && from.len() <= 21
        && from.bytes().all(|b| b.is_ascii_digit() || b == b'.' || b == b':');
    if ok { from.clone() } else { String::from("an unknown address") }
}

/// The context's name as a note shows it: its own, or `default`.
fn context_name(cx: &Cx<'_>) -> String {
    match session(cx).and_then(|s| s.context.as_deref()) {
        Some("") | None => String::from("default"),
        Some(name) => String::from(name),
    }
}

/// R80: the context's console goes to the login's channel, with a note saying how: none for a
/// new context, a reattachment, or a takeover naming the channel it was taken from. A fresh
/// attachment id names the channel from here; a new context's first is its session's id.
pub fn attach_relay(cx: &mut Cx<'_>) { attach_with(cx, true); }

/// The attach, and whether it clears the idle clock: always, but in the model's broken entry.
pub fn attach_with(cx: &mut Cx<'_>, clear_idle: bool) {
    let (from, name, reply, now) = (login_from(cx), context_name(cx), cx.reply, cx.now());
    let key = match &cx.event.kind {
        EventKind::Login { key, .. } => *key,
        _ => return,
    };
    let fresh = if session(cx).is_some_and(|s| s.attachment == 0) { cx.fresh() } else { 0 };
    let domain = cx.domain.clone();
    let Some(s) = cx.state.sessions.get_mut(&cx.id) else { return };
    let note = match (s.number, s.from.as_str()) {
        (0, _) => String::new(),
        (_, "") => format!("[context {name}: reattached]\r\n"),
        (_, old) => format!("[context {name}: reattached; taken over from {old}]\r\n"),
    };
    if s.attachment == 0 {
        s.attachment = fresh;
    }
    s.from = from;
    s.key = key;
    s.reply = reply;
    s.since = now;
    if clear_idle {
        s.idle = None;
    }
    let attachment = s.attachment;
    cx.index.attachments.insert(attachment, (domain, cx.id));
    let step = Step::Attach { relay: cx.token(RELAY), console: cx.token(CONSOLE), note };
    cx.step(step);
}

/// R80: the channel the context is attached to lets it go, told why if a login took it over;
/// its attachment id names nothing from here, so a close of that channel changes nothing.
pub fn detach_relay(cx: &mut Cx<'_>) { detach_with(cx, true); }

/// The detach, and whether the channel's attachment id is forgotten: always, but in the model's
/// broken entry.
pub fn detach_with(cx: &mut Cx<'_>, forget: bool) {
    let at = cx.now();
    let now = at / 1_000_000;
    let note = match &cx.event.kind {
        EventKind::Login { .. } => {
            let (from, name) = (login_from(cx), context_name(cx));
            format!(
                "[context {name} taken over from {from} at up {}h{:02}m]\r\n",
                now / 3600,
                now % 3600 / 60
            )
        }
        _ => String::new(),
    };
    let Some(s) = cx.state.sessions.get_mut(&cx.id) else { return };
    let attachment = core::mem::take(&mut s.attachment);
    s.since = at;
    s.idle.get_or_insert(at);
    if note.is_empty() {
        s.from.clear();
    }
    if forget {
        cx.index.attachments.remove(&attachment);
    }
    let step = Step::Detach { relay: cx.token(RELAY), console: cx.token(CONSOLE), note };
    cx.step(step);
}

/// A login, authenticated, names a context whose session lives: that session takes the login,
/// and this one, never started, ends here.
pub fn take_over(cx: &mut Cx<'_>) {
    let Some(name) = session(cx).and_then(|s| s.context.clone()) else { return };
    let live = cx.state.sessions.values().find(|o| {
        o.id != cx.id && o.state != crate::gen::session::State::Ending && o.context.as_ref() == Some(&name)
    });
    if let Some(o) = live {
        let object = Object { domain: cx.domain.clone(), kind: Kind::Session, id: o.id };
        cx.out.raised.push_back(Raised::Attach { object });
    }
}

/// A detached context past its idle bound ends: recorded, with how long it was idle.
pub fn audit_idle(cx: &mut Cx<'_>) {
    let now = cx.now();
    let Some(s) = session(cx) else { return };
    let idle = now.saturating_sub(s.idle.unwrap_or(now)) / 1_000_000;
    let record = Record::IdleEnded { session: s.id, idle };
    cx.audit(record);
}

/// A login to a context still starting is refused: it is in use, and nothing is attached.
pub fn refuse_in_use(cx: &mut Cx<'_>) { cx.reply(Answer::Refused(Refusal::InUse)); }

pub fn audit_attached(cx: &mut Cx<'_>) {
    let Some(s) = session(cx) else { return };
    let took_over = s.state == crate::gen::session::State::Running;
    let record = Record::Attached { session: s.id, key: s.key, from: s.from.clone(), took_over };
    cx.audit(record);
}

pub fn destroy_budget(cx: &mut Cx<'_>) {
    let step = Step::DestroyBudget { budget: cx.token(BUDGET) };
    cx.step(step);
}

/// Destroys what a failed batch made: its budget, if it made one (the embedder fails a destroy
/// of a token never made, and the object ends all the same).
pub fn destroy_partial(cx: &mut Cx<'_>) { destroy_budget(cx); }

/// Routes the badge to the object, which now runs, and numbers it in its domain (R37).
pub fn route(cx: &mut Cx<'_>) {
    let route = Route { domain: cx.domain.clone(), kind: cx.kind, id: cx.id };
    match cx.kind {
        Kind::Session => {
            cx.state.sessions_started += 1;
            let n = cx.state.sessions_started;
            if let Some(s) = cx.state.sessions.get_mut(&cx.id) {
                s.number = n;
                cx.index.routes.insert(s.badge, route);
            }
        }
        Kind::Lease => {
            cx.state.agents_started += 1;
            let n = cx.state.agents_started;
            if let Some(l) = cx.state.leases.get_mut(&cx.id) {
                l.number = n;
                cx.index.routes.insert(l.badge, route);
            }
        }
        _ => {}
    }
}

pub fn unroute(cx: &mut Cx<'_>) {
    let b = badge(cx);
    cx.index.routes.remove(&b);
}

/// A session's or an agent's end drops its requests.
pub fn drop_requests(cx: &mut Cx<'_>) {
    let mine: Vec<u64> = cx
        .state
        .requests
        .values()
        .filter(|r| r.by.kind == cx.kind && r.by.id == cx.id && !r.state.is_final())
        .map(|r| r.id)
        .collect();
    for id in mine {
        let request = Object { domain: cx.domain.clone(), kind: Kind::Request, id };
        cx.out.raised.push_back(Raised::SessionEnded { request });
    }
}

pub fn audit_login(cx: &mut Cx<'_>) {
    let Some(s) = session(cx) else { return };
    let record =
        Record::Login { session: s.id, principal: s.principal, key: s.key, context: s.context.clone() };
    cx.audit(record);
}

pub fn reply_login(cx: &mut Cx<'_>) {
    let Some(s) = session(cx) else { return };
    let answer = Answer::Session { id: s.attachment, name: format!("session-{}", s.number) };
    cx.reply(answer);
}

pub fn audit_agent_started(cx: &mut Cx<'_>) {
    let Some(l) = lease(cx) else { return };
    let record =
        Record::AgentStarted { lease: l.id, sponsor: l.principal, parent: l.parent, deadline: l.deadline };
    cx.audit(record);
}

pub fn audit_start_failed(cx: &mut Cx<'_>) {
    let id = cx.id;
    cx.audit(Record::StartFailed { lease: id });
}

pub fn reply_agent(cx: &mut Cx<'_>) {
    let Some(l) = lease(cx) else { return };
    let answer = Answer::Lease { id: l.id, name: format!("agent-{}", l.number) };
    cx.reply(answer);
}

/// A lease's end is audited with the lease's labels.
pub fn audit_lease_ended(cx: &mut Cx<'_>) {
    let id = cx.id;
    cx.audit(Record::LeaseEnded { lease: id });
}

/// The sponsor learns that the lease ended, not why: a notice to its unlabelled sessions.
pub fn notify_sponsor(cx: &mut Cx<'_>) {
    let sponsor = cx.domain.unlabelled();
    let to: Vec<u64> = cx
        .index
        .routes
        .iter()
        .filter(|(_, r)| r.kind == Kind::Session && r.domain == sponsor)
        .map(|(b, _)| *b)
        .collect();
    let lease = cx.id;
    for b in to {
        cx.output(Output::Notice { to: Notified::Session(b), notice: Notice::LeaseEnded { lease } });
    }
}

// Requests.

/// A declassification's read: a crossing in this, the labelled, domain.
pub fn open_read(cx: &mut Cx<'_>) {
    let Some(Content::Declassify { item, .. }) = request(cx).map(|r| r.content.clone()) else { return };
    let crossing = new_crossing(cx, CrossingKind::Read, item, Vec::new(), 0);
    let domain = cx.domain.clone();
    cx.out.raised.push_back(Raised::Open { domain, crossing });
}

fn new_crossing(cx: &mut Cx<'_>, kind: CrossingKind, item: u64, bytes: Vec<u8>, source: u64) -> Crossing {
    let id = cx.fresh();
    let request = cx.object();
    Crossing {
        id,
        state: crate::gen::crossing::State::Open,
        kind,
        request,
        item,
        bytes,
        source,
        through: false,
    }
}

/// A push's source, read by the steward itself: it is unlabelled.
pub fn read_source(cx: &mut Cx<'_>) {
    let Some(Content::Push { source, .. }) = request(cx).map(|r| r.content.clone()) else { return };
    let step = Step::Read { token: cx.token(READ), through: None, labels: Labels::empty(), item: source };
    cx.step(step);
}

/// The binding hash of the request's exact content.
pub fn freeze(cx: &mut Cx<'_>) {
    let domain = cx.domain.clone();
    if let Some(r) = cx.state.requests.get_mut(&cx.id) {
        r.hash = crate::hash::binding(&domain, r);
    }
}

pub fn audit_submitted(cx: &mut Cx<'_>) {
    let Some(r) = request(cx) else { return };
    let (domain, record) = (r.audit.clone(), Record::Submitted { request: r.id });
    cx.audit_in(&domain, record);
}

pub fn reply_request(cx: &mut Cx<'_>) {
    let id = cx.id;
    cx.reply(Answer::Request { id });
}

/// R38: an approval-waiting notice reaches only the sessions whose labels include all the
/// request's, and the principal's approval channels.
pub fn notify(cx: &mut Cx<'_>) {
    let Some(r) = request(cx) else { return };
    let principal = r.principal;
    let (account, labels) = (cx.domain.account(), cx.domain.labels().clone());
    let mut to: Vec<Notified> = cx
        .index
        .routes
        .iter()
        .filter(|(_, x)| x.domain.account() == account && x.domain.labels().includes(&labels))
        .map(|(b, _)| Notified::Session(*b))
        .collect();
    to.extend(
        cx.index.channels.values().filter(|c| c.principal == principal).map(|c| Notified::Channel(c.id)),
    );
    for n in to {
        cx.output(Output::Notice { to: n, notice: Notice::ApprovalWaiting });
    }
}

/// R38's screen, on the channel that asked.
pub fn render(cx: &mut Cx<'_>) {
    let Some(r) = request(cx) else { return };
    let screen = crate::render::screen(cx.fixed, cx.domain, cx.state, r, &crate::render::Rules::SHIPPED);
    show(cx, screen);
}

/// Shows `screen` on the channel that asked, which the request is now bound to.
pub fn show(cx: &mut Cx<'_>, screen: crate::effect::Rendered) {
    let Some(channel) = cx.channel else { return };
    if let Some(r) = cx.state.requests.get_mut(&cx.id) {
        r.channel = Some(channel);
    }
    cx.output(Output::Screen { channel, screen });
}

pub fn audit_approved(cx: &mut Cx<'_>) {
    let Some(r) = request(cx) else { return };
    let Some(c) = cx.channel.and_then(|c| cx.index.channels.get(&c)) else { return };
    let record = Record::Approved { request: r.id, principal: c.principal, key: c.key, hash: r.hash };
    let domain = r.audit.clone();
    cx.audit_in(&domain, record);
}

pub fn audit_denied(cx: &mut Cx<'_>) {
    let Some(r) = request(cx) else { return };
    let Some(c) = cx.channel.and_then(|c| cx.index.channels.get(&c)) else { return };
    let record = Record::Denied { request: r.id, principal: c.principal };
    let domain = r.audit.clone();
    cx.audit_in(&domain, record);
}

/// An approved labelled agent: a lease in its own domain.
pub fn grant_lease(cx: &mut Cx<'_>) {
    let Some(r) = request(cx) else { return };
    let (Content::Agent { lease, .. }, principal, domain) = (r.content.clone(), r.principal, r.audit.clone())
    else {
        return;
    };
    let id = cx.fresh();
    cx.out.raised.push_back(Raised::Granted { domain, id, principal, lease });
}

/// An approved declassification: its copy out, a crossing of this, the labelled, domain.
pub fn open_copy_out(cx: &mut Cx<'_>) {
    let Some(r) = request(cx) else { return };
    let (Content::Declassify { item, .. }, bytes, reader) = (r.content.clone(), r.snapshot.clone(), r.reader)
    else {
        return;
    };
    let crossing = new_crossing(cx, CrossingKind::CopyOut, item, bytes.unwrap_or_default(), reader);
    let domain = cx.domain.clone();
    cx.out.raised.push_back(Raised::Open { domain, crossing });
}

/// An approved push: its write, a crossing of the target's domain.
pub fn open_write(cx: &mut Cx<'_>) {
    let Some(r) = request(cx) else { return };
    let (Content::Push { source, item, .. }, bytes, domain) =
        (r.content.clone(), r.snapshot.clone(), r.audit.clone())
    else {
        return;
    };
    let crossing = new_crossing(cx, CrossingKind::Write, item, bytes.unwrap_or_default(), source);
    cx.out.raised.push_back(Raised::Open { domain, crossing });
}

// Crossings.

/// R42: a reader or writer budget carrying exactly the labelled side's labels, with a deadline.
pub fn carve_crossing(cx: &mut Cx<'_>) {
    if let Some(c) = cx.state.crossings.get_mut(&cx.id) {
        c.through = true;
    }
    let step = Step::CreateBudget {
        token: cx.token(BUDGET),
        parent: Parent::Users,
        limits: cx.fixed.sizes.crossing,
        labels: cx.domain.labels().clone(),
        deadline: Some(cx.now().saturating_add(CROSSING_LIFE)),
    };
    cx.step(step);
}

fn through(cx: &Cx<'_>) -> Option<Token> {
    cx.state.crossings.get(&cx.id).is_some_and(|c| c.through).then(|| cx.token(BUDGET))
}

/// The one item, read through the reader budget.
pub fn read_item(cx: &mut Cx<'_>) {
    let Some(item) = cx.state.crossings.get(&cx.id).map(|c| c.item) else { return };
    let step =
        Step::Read { token: cx.token(READ), through: through(cx), labels: cx.domain.labels().clone(), item };
    cx.step(step);
}

/// The one item, written through the writer budget.
pub fn write_item(cx: &mut Cx<'_>) {
    let Some((item, bytes)) = cx.state.crossings.get(&cx.id).map(|c| (c.item, c.bytes.clone())) else {
        return;
    };
    let step = Step::Write {
        through: through(cx),
        labels: cx.domain.labels().clone(),
        item,
        bytes: Bytes::Literal(bytes),
    };
    cx.step(step);
}

pub fn destroy_crossing(cx: &mut Cx<'_>) {
    if let Some(budget) = through(cx) {
        cx.step(Step::DestroyBudget { budget });
    }
}

/// R42: the copy out writes exactly the snapshot, and never reads the item again.
pub fn copy_out(cx: &mut Cx<'_>) {
    let Some((item, bytes)) = cx.state.crossings.get(&cx.id).map(|c| (c.item, c.bytes.clone())) else {
        return;
    };
    let step = Step::Write { through: None, labels: Labels::empty(), item, bytes: Bytes::Literal(bytes) };
    cx.step(step);
}

/// The read's snapshot, and the reader it came through, for the request.
pub fn pass_snapshot(cx: &mut Cx<'_>) {
    let Some(request) = cx.state.crossings.get(&cx.id).map(|c| c.request.clone()) else { return };
    let (bytes, reader) = cx.produced();
    let result = Some((bytes.unwrap_or_default(), reader));
    cx.out.raised.push_back(Raised::Snapshot { request, result });
}

pub fn pass_failure(cx: &mut Cx<'_>) {
    let Some(request) = cx.state.crossings.get(&cx.id).map(|c| c.request.clone()) else { return };
    cx.out.raised.push_back(Raised::Snapshot { request, result: None });
}

pub fn audit_declassified(cx: &mut Cx<'_>) {
    let Some(c) = cx.state.crossings.get(&cx.id) else { return };
    let record = Record::Declassified { request: c.request.id, bytes: c.bytes.clone(), reader: c.source };
    cx.audit(record);
}

pub fn audit_copy_failed(cx: &mut Cx<'_>) {
    let Some(request) = cx.state.crossings.get(&cx.id).map(|c| c.request.id) else { return };
    cx.audit(Record::CopyFailed { request });
}

pub fn audit_pushed(cx: &mut Cx<'_>) {
    let Some(c) = cx.state.crossings.get(&cx.id) else { return };
    let (request, source, item, bytes) = (c.request.id, c.source, c.item, c.bytes.clone());
    let writer = cx.produced().1;
    cx.audit(Record::Pushed { request, source, item, bytes, writer });
}

pub fn audit_push_failed(cx: &mut Cx<'_>) {
    let Some(request) = cx.state.crossings.get(&cx.id).map(|c| c.request.id) else { return };
    cx.audit(Record::PushFailed { request });
}

// Blame.

/// Keeps the latest `BLAME_COUNT` blames' times.
pub fn count_blame(cx: &mut Cx<'_>) {
    let now = cx.now();
    let times = &mut cx.state.blame.times;
    times.push(now);
    if times.len() > BLAME_COUNT {
        times.remove(0);
    }
}

pub fn audit_blamed(cx: &mut Cx<'_>) { cx.audit(Record::Blamed); }

/// R40: every session and lease of the domain ends, and none starts until the window passes.
pub fn lock_out(cx: &mut Cx<'_>) {
    let now = cx.now();
    cx.state.blame.until = now.saturating_add(BLAME_WINDOW);
    cx.state.blame.times.clear();
    let mut doomed: Vec<Object> = Vec::new();
    for id in cx.state.sessions.keys() {
        doomed.push(Object { domain: cx.domain.clone(), kind: Kind::Session, id: *id });
    }
    for id in cx.state.leases.keys() {
        doomed.push(Object { domain: cx.domain.clone(), kind: Kind::Lease, id: *id });
    }
    for object in doomed {
        cx.out.raised.push_back(Raised::LockedOut { object });
    }
}

pub fn audit_locked_out(cx: &mut Cx<'_>) {
    let until = cx.state.blame.until;
    cx.audit(Record::LockedOut { until });
}
