//! A trace's output, the core's side: after each event its effects in order, then the store
//! read through `inspect` (servers/steward.md, "The trace encoding").

use std::fmt::Write as _;

use redoubt_steward::audit::{Audit, Record};
use redoubt_steward::domain::Domain;
use redoubt_steward::effect::{
    Answer, Bytes, Effects, Kind, Notice, Notified, Object, Output, Parent, Server, Step, Token,
};
use redoubt_steward::event::Content;
use redoubt_steward::inspect;
use redoubt_steward::manifest::{Carve, Limits};
use redoubt_steward::store::{CrossingKind, Store};

use crate::text::{hex, quote, show_list};

pub fn domain(d: &Domain) -> String {
    let l: Vec<String> = d.labels().as_slice().iter().map(u64::to_string).collect();
    format!("{}/{}", d.account(), l.join(","))
}

fn kind(k: Kind) -> &'static str {
    match k {
        Kind::Session => "session",
        Kind::Lease => "lease",
        Kind::Request => "request",
        Kind::Crossing => "crossing",
        Kind::Blame => "blame",
        Kind::Channel => "channel",
    }
}

pub fn object(o: &Object) -> String { format!("{}@{}#{}", kind(o.kind), domain(&o.domain), o.id) }

fn token(t: &Token) -> String { format!("{}.{}", object(&t.owner), t.slot) }

fn limits(l: &Limits) -> String { format!("{},{},{}", l.pages, l.processes, l.weight) }

fn opt(o: Option<u64>) -> String { o.map_or("none".into(), |x| x.to_string()) }

/// A session's context: `none` for the console's, else its quoted name.
fn context_of(c: &Option<String>) -> String { c.as_ref().map_or("none".into(), |c| quote(c.as_bytes())) }

fn answer(a: &Answer) -> String {
    match a {
        Answer::Ok => "ok".into(),
        Answer::Session { id, name } => format!("session id={id} name={}", quote(name.as_bytes())),
        Answer::Lease { id, name } => format!("lease id={id} name={}", quote(name.as_bytes())),
        Answer::Request { id } => format!("request id={id}"),
        Answer::Refused(r) => format!("refused {r:?}"),
    }
}

fn record(r: &Record) -> String {
    match r {
        Record::Login { session, principal, key, context } => {
            format!("Login session={session} principal={principal} key={key} context={}", context_of(context))
        }
        Record::AgentStarted { lease, sponsor, parent, deadline } => {
            format!(
                "AgentStarted lease={lease} sponsor={sponsor} parent={} deadline={deadline}",
                opt(*parent)
            )
        }
        Record::StartFailed { lease } => format!("StartFailed lease={lease}"),
        Record::LeaseEnded { lease } => format!("LeaseEnded lease={lease}"),
        Record::Submitted { request } => format!("Submitted request={request}"),
        Record::Approved { request, principal, key, hash } => {
            format!("Approved request={request} principal={principal} key={key} hash={}", hex(hash))
        }
        Record::Denied { request, principal } => format!("Denied request={request} principal={principal}"),
        Record::Declassified { request, bytes, reader } => {
            format!("Declassified request={request} bytes={} reader={reader}", quote(bytes))
        }
        Record::CopyFailed { request } => format!("CopyFailed request={request}"),
        Record::Pushed { request, source, item, bytes, writer } => format!(
            "Pushed request={request} source={source} item={item} bytes={} writer={writer}",
            quote(bytes)
        ),
        Record::PushFailed { request } => format!("PushFailed request={request}"),
        Record::Blamed => "Blamed".into(),
        Record::LockedOut { until } => format!("LockedOut until={until}"),
    }
}

pub fn audit(a: &Audit) -> String {
    format!("audit {} at={} {}", domain(a.domain()), a.at(), record(a.record()))
}

fn step(s: &Step) -> String {
    let through = |t: &Option<Token>| t.as_ref().map_or("none".into(), token);
    match s {
        Step::CreateBudget { token: t, parent, limits: l, labels, deadline } => {
            let parent = match parent {
                Parent::Sub(d) => format!("sub({})", domain(d)),
                Parent::Budget(b) => format!("budget({})", token(b)),
                Parent::Users => "users".into(),
            };
            format!(
                "create-budget token={} parent={parent} limits={} labels={} deadline={}",
                token(t),
                limits(l),
                show_list(labels.as_slice()),
                opt(*deadline)
            )
        }
        Step::CreateScope { scope, budget } => {
            format!("create-scope scope={} budget={}", token(scope.token()), token(budget))
        }
        Step::Connect { token: t, scope, server, badge } => {
            let server = match server {
                Server::Steward => "steward".into(),
                Server::Shared(i) => format!("shared({i})"),
            };
            format!("connect token={} scope={} server={server} badge={badge}", token(t), token(scope.token()))
        }
        Step::Launch { token: t, budget, connections } => {
            let c: Vec<String> = connections.iter().map(token).collect();
            format!("launch token={} budget={} connections=[{}]", token(t), token(budget), c.join(","))
        }
        Step::DestroyBudget { budget } => format!("destroy-budget budget={}", token(budget)),
        Step::Read { token: t, through: th, labels, item } => format!(
            "read token={} through={} labels={} item={item}",
            token(t),
            through(th),
            show_list(labels.as_slice())
        ),
        Step::Write { through: th, labels, item, bytes } => {
            let bytes = match bytes {
                Bytes::Literal(b) => format!("literal({})", quote(b)),
                Bytes::Read(t) => format!("read({})", token(t)),
            };
            format!(
                "write through={} labels={} item={item} bytes={bytes}",
                through(th),
                show_list(labels.as_slice())
            )
        }
    }
}

/// One event's effects: its outputs in order, its batches in order, and `exit`.
pub fn effects(s: &mut String, e: &Effects) {
    for o in &e.outputs {
        let _ = match o {
            Output::Reply { to, answer: a } => writeln!(s, "reply {to} {}", answer(a)),
            Output::Notice { to, notice } => {
                let to = match to {
                    Notified::Session(b) => format!("session {b}"),
                    Notified::Channel(c) => format!("channel {c}"),
                };
                match notice {
                    Notice::ApprovalWaiting => writeln!(s, "notice {to} approval-waiting"),
                    Notice::LeaseEnded { lease } => writeln!(s, "notice {to} lease-ended lease={lease}"),
                }
            }
            Output::Screen { channel, screen } => writeln!(
                s,
                "screen {channel} id={} hash={} labels={} text={}",
                screen.id,
                hex(&screen.hash),
                show_list(screen.labels.as_slice()),
                quote(screen.text.as_bytes())
            ),
            Output::Audit(a) => writeln!(s, "{}", audit(a)),
            Output::Forget(o) => writeln!(s, "forget {}", object(o)),
        };
    }
    for b in &e.batches {
        let _ = writeln!(s, "batch {}", object(&b.owner));
        for st in &b.steps {
            let _ = writeln!(s, "step {}", step(st));
        }
    }
    if e.exit {
        s.push_str("exit\n");
    }
}

fn content(c: &Content) -> String {
    match c {
        Content::Note { what } => format!("note({})", quote(what.as_bytes())),
        Content::Agent { labels, lease } => format!("agent({},{lease})", show_list(labels)),
        Content::Declassify { labels, item } => format!("declassify({},{item})", show_list(labels)),
        Content::Push { source, target, item } => format!("push({source},{},{item})", show_list(target)),
    }
}

/// What boot fixed and carved, once, before the first event.
pub fn boot(s: &mut String, store: &Store, carves: &[Carve]) {
    let f = inspect::fixed(store);
    for (i, p) in f.principals.iter().enumerate() {
        let d: Vec<String> = p.domains.iter().map(domain).collect();
        let _ = writeln!(
            s,
            "principal {i} {} account={} login={} approval={} owned={} domains=[{}] top={}",
            quote(p.name.as_bytes()),
            p.account,
            show_list(&p.login_keys),
            show_list(&p.approval_keys),
            show_list(p.owned.as_slice()),
            d.join(","),
            limits(&p.top)
        );
    }
    let keyd: Vec<u64> = f.keyd.iter().copied().collect();
    let z = &f.sizes;
    let _ = writeln!(
        s,
        "fixed keyd={} servers={} sizes session={} agent={} sub_agent={} crossing={} cost={}",
        show_list(&keyd),
        f.servers,
        limits(&z.session),
        limits(&z.agent),
        limits(&z.sub_agent),
        limits(&z.crossing),
        z.budget_cost
    );
    for c in carves {
        let _ = writeln!(s, "carve account={} top={}", c.account, limits(&c.top));
        for (d, l) in &c.subs {
            let _ = writeln!(s, "carve-sub {} {}", domain(d), limits(l));
        }
    }
}

/// The store after an event: every domain in the manifest's order with its objects by id, then
/// the routing index and the approval channels, then the exited flag.
pub fn store(s: &mut String, store: &Store) {
    for (d, st) in inspect::domains(store) {
        let b = &st.blame;
        let _ = writeln!(
            s,
            "domain {} sessions_started={} agents_started={} blame={:?} times={} until={}",
            domain(d),
            st.sessions_started,
            st.agents_started,
            b.state,
            show_list(&b.times),
            b.until
        );
        for x in st.sessions.values() {
            let _ = writeln!(
                s,
                "  session id={} state={:?} principal={} key={} context={} badge={} number={} reply={}",
                x.id,
                x.state,
                x.principal,
                x.key,
                context_of(&x.context),
                x.badge,
                x.number,
                x.reply
            );
        }
        for x in st.leases.values() {
            let _ = writeln!(
                s,
                "  lease id={} state={:?} principal={} badge={} number={} lease={} deadline={} parent={} granted={} reply={}",
                x.id,
                x.state,
                x.principal,
                x.badge,
                x.number,
                x.lease,
                x.deadline,
                opt(x.parent),
                x.granted,
                x.reply
            );
        }
        for x in st.requests.values() {
            let _ = writeln!(
                s,
                "  request id={} state={:?} by={}#{} principal={} content={} reason={} snapshot={} reader={} hash={} audit={} channel={} reply={}",
                x.id,
                x.state,
                kind(x.by.kind),
                x.by.id,
                x.principal,
                content(&x.content),
                quote(x.reason.as_bytes()),
                x.snapshot.as_deref().map_or("none".into(), quote),
                x.reader,
                hex(&x.hash),
                domain(&x.audit),
                opt(x.channel),
                x.reply
            );
        }
        for x in st.crossings.values() {
            let k = match x.kind {
                CrossingKind::Read => "read",
                CrossingKind::CopyOut => "copy-out",
                CrossingKind::Write => "write",
            };
            let _ = writeln!(
                s,
                "  crossing id={} state={:?} kind={k} request={} item={} bytes={} source={} through={}",
                x.id,
                x.state,
                object(&x.request),
                x.item,
                quote(&x.bytes),
                x.source,
                x.through
            );
        }
    }
    let index = inspect::index(store);
    for (badge, r) in &index.routes {
        let _ = writeln!(s, "route {badge} {}@{}#{}", kind(r.kind), domain(&r.domain), r.id);
    }
    for (id, (d, k)) in &index.ids {
        let _ = writeln!(s, "id {id} {}@{}", kind(*k), domain(d));
    }
    for c in index.channels.values() {
        let _ =
            writeln!(s, "channel id={} state={:?} principal={} key={}", c.id, c.state, c.principal, c.key);
    }
    let _ = writeln!(s, "exited {}", inspect::exited(store));
}
