//! The policy core through `decide`, with a fake embedder that carries out every batch and
//! reports it: one test per guard and per effect that carries a rule (servers/steward.md,
//! "Guards and effects"), and the paths they sit on.

use std::collections::BTreeMap;

use redoubt_steward::audit::{Audit, Record};
use redoubt_steward::consts::{BLAME_WINDOW, DECLASSIFY_MAX, MAX_LEASE, PENDING_CAP, RANDOM_WORDS};
use redoubt_steward::domain::{Domain, Labels};
use redoubt_steward::effect::{
    Answer, Bytes, Kind, Notice, Notified, Object, Output, Parent, Produced, Refusal, Rendered, Step,
    StepFailed, Token,
};
use redoubt_steward::event::{Content, Event, EventKind};
use redoubt_steward::manifest::{Contexts, Limits, Manifest, PrincipalSpec, Sizes};
use redoubt_steward::{Policy, Store, decide, inspect};

const SECOND: u64 = 1_000_000;

fn manifest() -> Manifest {
    let p = |name: &str, account, login, approval, owned: &[u64], sets: &[&[u64]]| PrincipalSpec {
        name: name.into(),
        account,
        login_keys: vec![login],
        approval_keys: vec![approval],
        owned: owned.to_vec(),
        label_sets: sets.iter().map(|s| s.to_vec()).collect(),
        top: Limits { pages: 500, processes: 12, weight: 100 },
        // Bob's cap is the one the cap's tests meet; idle contexts end after five minutes.
        contexts: Contexts { max: if name == "bob" { 2 } else { 8 }, idle_secs: 300 },
    };
    let l = |pages, processes, weight| Limits { pages, processes, weight };
    Manifest {
        principals: vec![
            p("alice", 1001, 11, 12, &[7, 8], &[&[], &[7], &[8]]),
            // Bob works under {7} but does not own it.
            p("bob", 1002, 21, 22, &[9], &[&[], &[9], &[7]]),
            p("carol", 1003, 31, 32, &[], &[&[]]),
        ],
        keyd_keys: vec![100, 101],
        servers: 1,
        sizes: Sizes {
            session: l(40, 1, 5),
            agent: l(40, 2, 4),
            sub_agent: l(8, 1, 1),
            crossing: l(1, 0, 0),
            budget_cost: 1,
        },
    }
}

/// A write: (labels, item, bytes, the labels of the budget it went through).
type Write = (Vec<u64>, u64, Vec<u8>, Option<Vec<u64>>);

/// The fake embedder: it runs every batch at once, in order, and reports it.
struct Rig {
    store: Store,
    now: u64,
    word: u64,
    reply: u64,
    /// Volumes: (labels, item) -> bytes.
    volumes: BTreeMap<(Vec<u64>, u64), Vec<u8>>,
    /// Budgets the batches made: token -> (kernel id, labels).
    budgets: BTreeMap<Token, (u64, Vec<u64>)>,
    next_budget: u64,
    outputs: Vec<Output>,
    audit: Vec<Audit>,
    /// Writes, in order: (labels, item, bytes, the labels of the budget written through).
    writes: Vec<Write>,
    /// Reads: (labels, item, the labels of the budget read through).
    reads: Vec<(Vec<u64>, u64, Option<Vec<u64>>)>,
    /// Owners whose next batch fails at its first step.
    fail: Vec<Kind>,
    exited: bool,
    /// What each relay was told, in order: (the session, attached or not, the note).
    relayed: Vec<(u64, bool, String)>,
    /// The attachment the last login's answer named.
    attachment: u64,
    /// The contexts `login` has named, so that each of its logins is a context of its own.
    contexts: u64,
}

fn d(account: u64, labels: &[u64]) -> Domain {
    Domain::new(account.try_into().unwrap(), Labels::new(labels).unwrap())
}

impl Rig {
    fn new() -> Rig { Rig::with(Policy::SHIPPED) }

    fn with(policy: Policy) -> Rig {
        let (store, carves) = Store::boot(&manifest(), policy).expect("the manifest is valid");
        assert_eq!(carves.len(), 3);
        Rig {
            store,
            now: SECOND,
            word: 0,
            reply: 0,
            volumes: BTreeMap::new(),
            budgets: BTreeMap::new(),
            next_budget: 1000,
            outputs: Vec::new(),
            audit: Vec::new(),
            writes: Vec::new(),
            reads: Vec::new(),
            fail: Vec::new(),
            exited: false,
            relayed: Vec::new(),
            attachment: 0,
            contexts: 0,
        }
    }

    fn words(&mut self) -> [u64; RANDOM_WORDS] {
        let mut w = [0; RANDOM_WORDS];
        for x in &mut w {
            self.word += 1;
            *x = self.word.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        }
        w
    }

    /// One event with a fresh reply token, and every batch it starts run to the end; the
    /// answer to the event's call, if any.
    fn call(&mut self, kind: EventKind) -> Option<Answer> {
        self.reply += 1;
        let reply = self.reply;
        let from = self.outputs.len();
        self.feed(reply, kind);
        self.outputs[from..].iter().find_map(|o| match o {
            Output::Reply { to, answer } if *to == reply => Some(answer.clone()),
            _ => None,
        })
    }

    fn feed(&mut self, reply: u64, kind: EventKind) {
        let mut queue = vec![(reply, kind)];
        while let Some((reply, kind)) = queue.pop() {
            let random = self.words();
            let e = decide(&mut self.store, Event { now: self.now, random, reply, kind });
            if e.exit {
                self.exited = true;
                return;
            }
            for o in &e.outputs {
                if let Output::Audit(a) = o {
                    self.audit.push(a.clone());
                }
            }
            self.outputs.extend(e.outputs);
            for b in e.batches {
                let result = self.run(&b.owner, &b.steps);
                queue.insert(0, (0, EventKind::Done { object: b.owner, result }));
            }
        }
    }

    fn labels_of(&self, through: &Option<Token>) -> Option<Vec<u64>> {
        through.as_ref().and_then(|t| self.budgets.get(t)).map(|b| b.1.clone())
    }

    fn run(&mut self, owner: &Object, steps: &[Step]) -> Result<Vec<Produced>, StepFailed> {
        if let Some(i) = self.fail.iter().position(|k| *k == owner.kind) {
            self.fail.remove(i);
            return Err(StepFailed { step: 0, error: 1 });
        }
        let mut done = Vec::new();
        let mut read: BTreeMap<Token, Vec<u8>> = BTreeMap::new();
        for (i, s) in steps.iter().enumerate() {
            let p = match s {
                Step::CreateBudget { token, labels, .. } => {
                    self.next_budget += 1;
                    self.budgets.insert(token.clone(), (self.next_budget, labels.as_slice().to_vec()));
                    Produced::Budget(self.next_budget)
                }
                Step::CreateScope { .. } => Produced::Scope,
                Step::Connect { .. } => Produced::Connection,
                Step::Launch { .. } | Step::LaunchRelay { .. } => Produced::Process(1),
                Step::Attach { note, .. } => {
                    self.relayed.push((owner.id, true, note.clone()));
                    Produced::Done
                }
                Step::Detach { note, .. } => {
                    self.relayed.push((owner.id, false, note.clone()));
                    Produced::Done
                }
                Step::DestroyBudget { budget } => {
                    if self.budgets.remove(budget).is_none() {
                        return Err(StepFailed { step: i, error: 2 });
                    }
                    Produced::Done
                }
                Step::Read { token, through, labels, item } => {
                    let through = self.labels_of(through);
                    self.reads.push((labels.as_slice().to_vec(), *item, through));
                    let b =
                        self.volumes.get(&(labels.as_slice().to_vec(), *item)).cloned().unwrap_or_default();
                    read.insert(token.clone(), b.clone());
                    Produced::Bytes(b)
                }
                Step::Write { through, labels, item, bytes } => {
                    let b = match bytes {
                        Bytes::Literal(b) => b.clone(),
                        Bytes::Read(t) => read.get(t).cloned().unwrap_or_default(),
                    };
                    let through = self.labels_of(through);
                    self.writes.push((labels.as_slice().to_vec(), *item, b.clone(), through));
                    self.volumes.insert((labels.as_slice().to_vec(), *item), b);
                    Produced::Done
                }
            };
            done.push(p);
        }
        Ok(done)
    }

    /// A login as a context no other login of the rig has named.
    fn login(&mut self, name: &str, labels: &[u64], key: u64) -> Result<(u64, u64), Answer> {
        self.contexts += 1;
        let context = format!("c{}", self.contexts);
        self.login_as(name, labels, &context, key)
    }

    fn login_as(
        &mut self,
        name: &str,
        labels: &[u64],
        context: &str,
        key: u64,
    ) -> Result<(u64, u64), Answer> {
        let kind = EventKind::Login {
            principal: name.into(),
            labels: labels.to_vec(),
            context: context.into(),
            key,
            from: String::from("198.51.100.2:51234"),
        };
        match self.call(kind) {
            Some(Answer::Session { id, .. }) => {
                // A takeover's or a reattachment's id is a fresh attachment of the live session.
                self.attachment = id;
                let session = inspect::index(&self.store).attachments.get(&id).map_or(id, |(_, s)| *s);
                Ok((session, self.badge(session)))
            }
            other => Err(other.unwrap_or(Answer::Ok)),
        }
    }

    /// A session's or a lease's badge.
    fn badge(&self, id: u64) -> u64 {
        inspect::index(&self.store).routes.iter().find(|(_, r)| r.id == id).map(|(b, _)| *b).unwrap()
    }

    fn agent(&mut self, badge: u64, lease: u64) -> Result<(u64, u64), Answer> {
        match self.call(EventKind::StartAgent { badge, lease }) {
            Some(Answer::Lease { id, .. }) => Ok((id, self.badge(id))),
            other => Err(other.unwrap_or(Answer::Ok)),
        }
    }

    fn submit(&mut self, badge: u64, content: Content, reason: &str) -> Result<u64, Answer> {
        match self.call(EventKind::Submit { badge, content, reason: reason.into() }) {
            Some(Answer::Request { id }) => Ok(id),
            other => Err(other.unwrap_or(Answer::Ok)),
        }
    }

    fn open(&mut self, channel: u64, name: &str, key: u64) -> Option<Answer> {
        self.call(EventKind::ApprovalOpened { channel, principal: name.into(), key })
    }

    /// The screens a channel gets for `Pending`.
    fn pending(&mut self, channel: u64) -> Vec<Rendered> {
        let from = self.outputs.len();
        self.call(EventKind::Pending { channel });
        self.outputs[from..]
            .iter()
            .filter_map(|o| match o {
                Output::Screen { channel: c, screen } if *c == channel => Some(screen.clone()),
                _ => None,
            })
            .collect()
    }

    fn hash(&self, request: u64) -> [u8; 32] {
        inspect::domains(&self.store).find_map(|(_, s)| s.requests.get(&request)).map(|r| r.hash).unwrap()
    }

    fn approve(&mut self, channel: u64, request: u64) -> Option<Answer> {
        let hash = self.hash(request);
        self.call(EventKind::Approve { channel, request, hash })
    }

    fn pending_in(&self, domain: &Domain) -> usize {
        inspect::domain(&self.store, domain).unwrap().requests.len()
    }

    fn sessions_in(&self, domain: &Domain) -> usize {
        let s = inspect::domain(&self.store, domain).unwrap();
        s.sessions.len() + s.leases.len()
    }

    fn blame(&mut self, account: u64, labels: &[u64]) {
        self.call(EventKind::Blame { account, labels: labels.to_vec() });
    }

    fn records(&self, f: impl Fn(&Record) -> bool) -> Vec<&Audit> {
        self.audit.iter().filter(|a| f(a.record())).collect()
    }

    fn notices(&self, from: usize) -> Vec<(Notified, Notice)> {
        self.outputs[from..]
            .iter()
            .filter_map(|o| match o {
                Output::Notice { to, notice } => Some((*to, *notice)),
                _ => None,
            })
            .collect()
    }
}

fn refused(a: Result<impl std::fmt::Debug, Answer>) -> Refusal {
    match a {
        Err(Answer::Refused(r)) => r,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn refusal(a: Option<Answer>) -> Refusal {
    match a {
        Some(Answer::Refused(r)) => r,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_manifest_with_a_key_in_two_roles_is_refused() {
    let mut m = manifest();
    m.principals[1].approval_keys.push(11);
    assert!(Store::boot(&m, Policy::SHIPPED).is_none());
    let mut m = manifest();
    m.keyd_keys.push(31);
    assert!(Store::boot(&m, Policy::SHIPPED).is_none());
    let mut m = manifest();
    m.principals[2].account = 0;
    assert!(Store::boot(&m, Policy::SHIPPED).is_none());
}

#[test]
fn boot_carves_a_fixed_sub_budget_per_label_set() {
    let (_, carves) = Store::boot(&manifest(), Policy::SHIPPED).unwrap();
    let alice = &carves[0];
    assert_eq!(alice.subs.len(), 3);
    assert!(alice.subs.iter().all(|(_, l)| *l == Limits { pages: 500 / 3 - 1, processes: 4, weight: 33 }));
}

#[test]
fn login_key() {
    let mut r = Rig::new();
    let (id, _) = r.login("alice", &[], 11).unwrap();
    assert_ne!(id, 0);
    assert_eq!(refused(r.login("alice", &[], 12)), Refusal::BadKey, "an approval key");
    assert_eq!(refused(r.login("alice", &[], 21)), Refusal::BadKey, "bob's key");
    assert_eq!(refused(r.login("alice", &[], 100)), Refusal::BadKey, "a key keyd holds");
    assert_eq!(refused(r.login("dave", &[], 11)), Refusal::BadKey, "a principal the manifest does not name");
    assert_eq!(r.records(|x| matches!(x, Record::Login { .. })).len(), 1);
}

#[test]
fn owns_labels_reads_the_manifest_not_the_domains() {
    let mut r = Rig::new();
    r.login("alice", &[7], 11).unwrap();
    // A label set the manifest does not give alice: no domain, the same answer as a wrong key.
    assert_eq!(refused(r.login("alice", &[9], 11)), Refusal::BadKey);
    // Bob works under {7} but does not own it: the domain exists, the guard refuses, alike.
    assert_eq!(refused(r.login("bob", &[7], 21)), Refusal::BadKey);
    assert_eq!(r.sessions_in(&d(1002, &[7])), 0);
}

/// R79's refusals before a session: whatever a login names wrongly, the answer is the wrong key's,
/// and nothing is carved or recorded.
#[test]
fn a_login_s_refusals_tell_nothing_apart() {
    let mut r = Rig::new();
    let bad = [
        ("alice", &[][..], "c", 12, "a wrong key"),
        ("dave", &[][..], "c", 11, "an unknown principal"),
        ("alice", &[9][..], "c", 11, "a label set not given"),
        ("bob", &[7][..], "c", 21, "a label set not owned"),
        ("alice", &[][..], "Work", 11, "a context with upper case"),
        ("alice", &[][..], "a.b", 11, "a context with a dot"),
        ("alice", &[][..], "a:b", 11, "a context with a colon"),
        ("alice", &[][..], "1a", 11, "a context starting with a digit"),
        ("approve", &[][..], "", 11, "a reserved name"),
        ("", &[][..], "", 11, "a user name that is not a login"),
    ];
    let long = "a".repeat(65);
    for (who, labels, context, key, what) in
        bad.iter().copied().chain([("alice", &[][..], &*long, 11, "65 bytes")])
    {
        assert_eq!(refused(r.login_as(who, labels, context, key)), Refusal::BadKey, "{what}");
    }
    assert!(r.budgets.is_empty());
    assert!(r.records(|x| matches!(x, Record::Login { .. })).is_empty());
    r.login_as("alice", &[], &"a".repeat(64), 11).unwrap();
}

/// R79: one session per context name in a domain; the name is the domain's, so the same name in
/// another label set, or of another principal, is another context. A login naming a live one
/// attaches to it instead of starting a second.
#[test]
fn context_free_holds_one_session_per_name() {
    let mut r = Rig::new();
    let (work, _) = r.login_as("alice", &[], "work", 11).unwrap();
    let (default, _) = r.login_as("alice", &[], "", 11).unwrap();
    let budgets = r.budgets.len();
    assert_eq!(r.login_as("alice", &[], "work", 11).unwrap().0, work, "the same session");
    assert_eq!(r.login_as("alice", &[], "", 11).unwrap().0, default, "the default context too");
    assert_eq!(r.budgets.len(), budgets, "nothing carved");
    // Another label set's `work`, and bob's, are not alice's unlabelled `work`.
    r.login_as("alice", &[7], "work", 11).unwrap();
    r.login_as("bob", &[], "work", 21).unwrap();
    assert_eq!(r.budgets.len(), budgets + 2);
    // Once the session has ended, the name is free again: a new session.
    let badge = r.badge(work);
    r.call(EventKind::EndSession { badge });
    let (again, _) = r.login_as("alice", &[], "work", 11).unwrap();
    assert_ne!(again, work);
    let contexts: Vec<_> = r
        .records(|x| matches!(x, Record::Login { .. }))
        .into_iter()
        .filter_map(|a| match a.record() {
            Record::Login { context, .. } => context.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(contexts, ["work", "", "work", "work", "work"]);
}

/// R80: a login to an attached context takes it over, both channels told; the channel it was
/// taken from closes and changes nothing; the attached one's close detaches it; the next login
/// reattaches; `sshd`'s end detaches every context, never the console's session.
#[test]
fn a_context_is_attached_to_one_channel_at_a_time() {
    let mut r = Rig::new();
    let (work, _) = r.login_as("alice", &[], "work", 11).unwrap();
    let first = r.attachment;
    assert_eq!(first, work, "a new context's first attachment is its session's id");
    assert_eq!(r.relayed, [(work, true, String::new())]);
    r.now = 7_980 * SECOND;
    assert_eq!(r.login_as("alice", &[], "work", 11).unwrap().0, work);
    let second = r.attachment;
    assert_ne!(second, first);
    assert_eq!(
        r.relayed[1..],
        [
            (work, false, String::from("[context work taken over from 198.51.100.2:51234 at up 2h13m]\r\n")),
            (work, true, String::from("[context work: reattached; taken over from 198.51.100.2:51234]\r\n")),
        ]
    );
    let took: Vec<_> =
        r.records(|x| matches!(x, Record::Attached { took_over: true, .. })).into_iter().collect();
    assert_eq!(took.len(), 1);
    // The channel taken from closes: nothing changes.
    r.relayed.clear();
    assert_eq!(r.call(EventKind::ChannelClosed { session: first }), Some(Answer::Refused(Refusal::Unknown)));
    assert!(r.relayed.is_empty());
    assert_eq!(inspect::domain(&r.store, &d(1001, &[])).unwrap().sessions[&work].attachment, second);
    // A wrong key never reaches the context.
    assert_eq!(refused(r.login_as("alice", &[], "work", 12)), Refusal::BadKey);
    assert!(r.relayed.is_empty());
    // The attached channel closes: detached, and its id names nothing from here.
    r.call(EventKind::ChannelClosed { session: second });
    assert_eq!(r.relayed, [(work, false, String::new())]);
    assert_eq!(r.call(EventKind::ChannelClosed { session: second }), Some(Answer::Refused(Refusal::Unknown)));
    r.login_as("alice", &[], "work", 11).unwrap();
    assert_eq!(r.relayed[1..], [(work, true, String::from("[context work: reattached]\r\n"))]);
    // `sshd` is gone: every context detaches, and the console's session stays as it is.
    r.relayed.clear();
    r.login_as("bob", &[], "", 21).unwrap();
    r.call(EventKind::SshdGone);
    let detached = r.relayed.iter().filter(|x| !x.1).count();
    assert_eq!(detached, 2);
    assert!(inspect::index(&r.store).attachments.is_empty());
}

/// Rule 6: a login that would make one more context than the principal's cap in its domain is
/// refused `cap`, after its key is checked, and makes nothing; the oldest is never evicted, a
/// login to a live context is never capped, and another label set counts its own.
#[test]
fn the_cap_refuses_a_new_context_and_keeps_the_oldest() {
    let mut r = Rig::new();
    let (a, _) = r.login_as("bob", &[], "a", 21).unwrap();
    let (b, _) = r.login_as("bob", &[], "b", 21).unwrap();
    r.call(EventKind::ChannelClosed { session: b });
    let made = r.budgets.len();
    assert_eq!(refused(r.login_as("bob", &[], "c", 21)), Refusal::Cap);
    assert_eq!(r.budgets.len(), made, "nothing carved");
    // A wrong key is still the bad key's, never the cap's.
    assert_eq!(refused(r.login_as("bob", &[], "c", 22)), Refusal::BadKey);
    let live = &inspect::domain(&r.store, &d(1002, &[])).unwrap().sessions;
    assert!(live.contains_key(&a) && live.contains_key(&b), "the oldest stays");
    // The live ones still reattach or take over.
    assert_eq!(r.login_as("bob", &[], "b", 21).unwrap().0, b);
    assert_eq!(r.login_as("bob", &[], "a", 21).unwrap().0, a);
    // Another label set has a cap of its own.
    r.login_as("bob", &[9], "c", 21).unwrap();
    // One ends: a new one fits again.
    let badge = r.badge(a);
    r.call(EventKind::EndSession { badge });
    r.login_as("bob", &[], "c", 21).unwrap();
}

/// A detached context ends once its idle clock passes the principal's bound; an attached one
/// never does, however long ago it last detached, since an attach clears the clock; the clock
/// starts again at the next detach.
#[test]
fn idle_ends_a_detached_context_only() {
    let mut r = Rig::new();
    let (x, _) = r.login_as("alice", &[], "x", 11).unwrap();
    let (y, _) = r.login_as("alice", &[], "y", 11).unwrap();
    r.call(EventKind::ChannelClosed { session: x });
    r.call(EventKind::ChannelClosed { session: y });
    assert_eq!(r.store.next_idle(), Some(r.now + 300 * SECOND));
    // y reattaches before its bound, then detaches again later.
    r.now += 200 * SECOND;
    r.login_as("alice", &[], "y", 11).unwrap();
    r.now += 200 * SECOND;
    r.call(EventKind::Idle);
    let live = &inspect::domain(&r.store, &d(1001, &[])).unwrap().sessions;
    assert!(!live.contains_key(&x), "x, detached for 400 s, ended");
    assert!(live.contains_key(&y), "y is attached");
    assert_eq!(r.records(|x| matches!(x, Record::IdleEnded { idle: 400, .. })).len(), 1);
    let attachment = r.attachment;
    r.call(EventKind::ChannelClosed { session: attachment });
    r.now += 299 * SECOND;
    r.call(EventKind::Idle);
    assert!(
        inspect::domain(&r.store, &d(1001, &[])).unwrap().sessions.contains_key(&y),
        "its clock restarted"
    );
    r.now += SECOND;
    r.call(EventKind::Idle);
    assert!(!inspect::domain(&r.store, &d(1001, &[])).unwrap().sessions.contains_key(&y));
    assert_eq!(r.store.next_idle(), None);
}

/// A session lists, detaches and ends the contexts of its own domain only: another label set's
/// and another principal's are not listed, and ending one is the same `unknown` as a name
/// nobody holds. Its own detach lets its channel go; the console's session has none to let go.
#[test]
fn a_session_sees_and_ends_its_own_domain_s_contexts_only() {
    let mut r = Rig::new();
    let (work, _) = r.login_as("alice", &[], "work", 11).unwrap();
    let (home, _) = r.login_as("alice", &[], "home", 11).unwrap();
    r.login_as("alice", &[7], "vault", 11).unwrap();
    r.login_as("bob", &[], "work2", 21).unwrap();
    r.call(EventKind::ChannelClosed { session: work });
    r.now += 5 * SECOND;
    let badge = r.badge(home);
    let Some(Answer::Contexts(list)) = r.call(EventKind::Contexts { badge }) else { panic!() };
    let names: Vec<(&str, bool, u64)> = list.iter().map(|c| (c.name.as_str(), c.attached, c.age)).collect();
    assert_eq!(names, [("home", true, 5 * SECOND), ("work", false, 5 * SECOND)]);
    for name in ["vault", "work2", "nobody"] {
        let e = EventKind::EndContext { badge, name: name.into() };
        assert_eq!(refusal(r.call(e)), Refusal::Unknown, "{name}");
    }
    assert_eq!(r.call(EventKind::EndContext { badge, name: "work".into() }), Some(Answer::Ok));
    assert!(!inspect::domain(&r.store, &d(1001, &[])).unwrap().sessions.contains_key(&work));
    // Its own detach: the channel goes, the context runs on.
    r.relayed.clear();
    assert_eq!(r.call(EventKind::Leave { badge }), Some(Answer::Ok));
    assert_eq!(r.relayed, [(home, false, String::new())]);
    assert_eq!(refusal(r.call(EventKind::Leave { badge })), Refusal::Unknown, "already detached");
    // Its own end: as `exit`.
    assert_eq!(r.call(EventKind::EndContext { badge, name: "home".into() }), Some(Answer::Ok));
    // The console's session is no context: it lists none of itself and has no channel to let go.
    r.call(EventKind::Console { principal: "alice".into() });
    let console = inspect::domain(&r.store, &d(1001, &[]))
        .unwrap()
        .sessions
        .values()
        .find(|s| s.context.is_none())
        .map(|s| s.id)
        .unwrap();
    let badge = r.badge(console);
    assert_eq!(r.call(EventKind::Contexts { badge }), Some(Answer::Contexts(vec![])));
    assert_eq!(refusal(r.call(EventKind::Leave { badge })), Refusal::Unknown, "the console's");
}

#[test]
fn a_session_is_carved_from_its_domain_with_a_scope_for_its_connections() {
    let mut r = Rig::new();
    let (id, badge) = r.login("alice", &[7], 11).unwrap();
    let s = &inspect::domain(&r.store, &d(1001, &[7])).unwrap().sessions[&id];
    assert_eq!((s.badge, s.number), (badge, 1));
    // Numbered per domain: alice's unlabelled sessions start from 1 too.
    let (u, _) = r.login("alice", &[], 11).unwrap();
    assert_eq!(inspect::domain(&r.store, &d(1001, &[])).unwrap().sessions[&u].number, 1);
}

#[test]
fn the_batch_steps_for_a_session() {
    let (mut store, _) = Store::boot(&manifest(), Policy::SHIPPED).unwrap();
    let kind = EventKind::Login {
        principal: "alice".into(),
        labels: vec![7],
        context: String::new(),
        key: 11,
        from: String::new(),
    };
    let e = decide(&mut store, Event { now: 1, random: [5, 6, 7, 8, 9, 10, 11, 12], reply: 1, kind });
    assert_eq!(e.batches.len(), 1);
    let steps = &e.batches[0].steps;
    assert!(matches!(&steps[0], Step::CreateBudget { parent: Parent::Sub(dom), labels, deadline: None, .. }
        if *dom == d(1001, &[7]) && labels.as_slice() == [7]));
    assert!(matches!(&steps[1], Step::CreateScope { .. }));
    // Every connection is narrowed to that scope, never to the session's budget (R41).
    let Step::CreateScope { scope, .. } = &steps[1] else { unreachable!() };
    let conns: Vec<_> = steps.iter().filter(|s| matches!(s, Step::Connect { .. })).collect();
    assert_eq!(conns.len(), 2);
    assert!(conns.iter().all(|s| matches!(s, Step::Connect { scope: sc, .. } if sc == scope)));
    assert!(matches!(steps.last(), Some(Step::Launch { .. })));
    // The ids are the event's random words (R36).
    assert_eq!(e.batches[0].owner.id, 5);
}

#[test]
fn approval_key() {
    let mut r = Rig::new();
    assert_eq!(r.open(1, "alice", 12), Some(Answer::Ok));
    assert_eq!(refusal(r.open(2, "alice", 11)), Refusal::BadKey, "a login key");
    assert_eq!(refusal(r.open(3, "alice", 100)), Refusal::BadKey, "a key keyd holds");
    assert_eq!(refusal(r.open(4, "alice", 22)), Refusal::BadKey, "bob's");
    assert_eq!(refusal(r.open(1, "alice", 12)), Refusal::Unknown, "the same channel twice");
    assert_eq!(inspect::index(&r.store).channels.len(), 1);
    r.call(EventKind::ApprovalClosed { channel: 1 });
    assert!(inspect::index(&r.store).channels.is_empty());
}

#[test]
fn caller_unlabelled() {
    let mut r = Rig::new();
    let (_, vault) = r.login("alice", &[7], 11).unwrap();
    assert_eq!(refused(r.agent(vault, SECOND)), Refusal::Labelled);
    let (_, plain) = r.login("alice", &[], 11).unwrap();
    assert!(r.agent(plain, SECOND).is_ok());
}

#[test]
fn lease_bounded_and_carve_lease() {
    let mut r = Rig::new();
    let (_, b) = r.login("alice", &[], 11).unwrap();
    assert_eq!(refused(r.agent(b, 0)), Refusal::BadLease);
    assert_eq!(refused(r.agent(b, MAX_LEASE + 1)), Refusal::BadLease);
    assert_eq!(refused(r.agent(b, u64::MAX)), Refusal::BadLease);
    let (agent, ab) = r.agent(b, MAX_LEASE).unwrap();
    let u = d(1001, &[]);
    let deadline = inspect::domain(&r.store, &u).unwrap().leases[&agent].deadline;
    assert_eq!(deadline, r.now + MAX_LEASE);
    // A sub-agent sits inside its agent's budget and ends no later.
    let (sub, _) = r.agent(ab, MAX_LEASE).unwrap();
    let l = &inspect::domain(&r.store, &u).unwrap().leases[&sub];
    assert_eq!((l.parent, l.deadline), (Some(agent), deadline));
    assert_eq!(r.records(|x| matches!(x, Record::AgentStarted { parent: Some(_), .. })).len(), 1);
}

#[test]
fn blame_window_and_not_locked() {
    let mut r = Rig::new();
    r.login("alice", &[7], 11).unwrap();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    r.agent(u, SECOND).unwrap();
    let vault = d(1001, &[7]);
    // Two blames, then one outside the window: no lockout.
    r.blame(1001, &[7]);
    r.blame(1001, &[7]);
    r.now += BLAME_WINDOW;
    r.blame(1001, &[7]);
    assert_eq!(r.sessions_in(&vault), 1);
    // Two more inside the window: three within ten minutes.
    r.now += SECOND;
    r.blame(1001, &[7]);
    assert_eq!(r.sessions_in(&vault), 1);
    r.blame(1001, &[7]);
    assert_eq!(r.sessions_in(&vault), 0, "the vault's sessions end");
    assert_eq!(r.sessions_in(&d(1001, &[])), 2, "the unlabelled side is untouched");
    assert_eq!(refused(r.login("alice", &[7], 11)), Refusal::LockedOut);
    assert_eq!(r.records(|x| matches!(x, Record::LockedOut { .. })).len(), 1);
    assert!(r.records(|x| matches!(x, Record::Blamed)).iter().all(|a| *a.domain() == vault));
    r.now += BLAME_WINDOW;
    assert!(r.login("alice", &[7], 11).is_ok());
    // Account 0 and an unknown label set blame nobody.
    r.blame(0, &[]);
    r.blame(1001, &[42]);
    assert_eq!(r.records(|x| matches!(x, Record::Blamed)).len(), 5);
}

#[test]
fn pending_cap_and_fair_share() {
    let mut r = Rig::new();
    let (_, a) = r.login("carol", &[], 31).unwrap();
    let note = || Content::Note { what: "x".into() };
    for _ in 0..PENDING_CAP {
        r.submit(a, note(), "").unwrap();
    }
    assert_eq!(refused(r.submit(a, note(), "")), Refusal::Cap);
    // A second session: each holds at most half the cap, and the domain the cap.
    let (_, b) = r.login("carol", &[], 31).unwrap();
    assert_eq!(refused(r.submit(b, note(), "")), Refusal::Cap);
    let mut r = Rig::new();
    let (_, a) = r.login("carol", &[], 31).unwrap();
    let (_, b) = r.login("carol", &[], 31).unwrap();
    r.submit(a, note(), "").unwrap();
    r.submit(a, note(), "").unwrap();
    assert_eq!(refused(r.submit(a, note(), "")), Refusal::Cap, "over its fair share");
    r.submit(b, note(), "").unwrap();
    // Another domain of the same account has a cap of its own.
    let mut r = Rig::new();
    let (_, v) = r.login("alice", &[7], 11).unwrap();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    for _ in 0..PENDING_CAP {
        r.submit(v, note(), "").unwrap();
    }
    r.submit(u, note(), "").unwrap();
}

#[test]
fn drop_requests() {
    let mut r = Rig::new();
    let (s, b) = r.login("carol", &[], 31).unwrap();
    r.submit(b, Content::Note { what: "x".into() }, "").unwrap();
    assert_eq!(r.pending_in(&d(1003, &[])), 1);
    r.call(EventKind::EndSession { badge: b });
    assert_eq!(r.pending_in(&d(1003, &[])), 0);
    assert_eq!(r.sessions_in(&d(1003, &[])), 0);
    // Its badge routes nothing any more.
    assert_eq!(refused(r.submit(b, Content::Note { what: "x".into() }, "")), Refusal::Unknown);
}

#[test]
fn exact_labels_and_item_fits() {
    let mut r = Rig::new();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    let (_, v) = r.login("alice", &[7], 11).unwrap();
    let declassify = |item| Content::Declassify { labels: vec![7], item };
    assert_eq!(refused(r.submit(u, declassify(1), "")), Refusal::NotOwner, "from an unlabelled session");
    let push = Content::Push { source: 1, target: vec![7], item: 1 };
    assert_eq!(refused(r.submit(v, push, "")), Refusal::NotOwner, "a push from a labelled session");
    let unlabelled = Content::Declassify { labels: vec![], item: 1 };
    assert_eq!(refused(r.submit(u, unlabelled, "")), Refusal::NotOwner, "an unlabelled item declassified");
    let unlabelled = Content::Push { source: 1, target: vec![], item: 1 };
    assert_eq!(refused(r.submit(u, unlabelled, "")), Refusal::NotOwner, "an unlabelled item pushed");
    r.volumes.insert((vec![7], 2), vec![b'a'; DECLASSIFY_MAX + 1]);
    r.volumes.insert((vec![7], 3), vec![b'a', 0x1b]);
    r.volumes.insert((vec![7], 4), vec![b'a'; DECLASSIFY_MAX]);
    assert_eq!(refused(r.submit(v, declassify(2), "")), Refusal::TooBig);
    assert_eq!(refused(r.submit(v, declassify(3), "")), Refusal::NotPrintable);
    assert!(r.submit(v, declassify(4), "").is_ok());
    // A pushed item is not capped.
    r.volumes.insert((vec![], 5), vec![0; DECLASSIFY_MAX * 4]);
    assert!(r.submit(u, Content::Push { source: 5, target: vec![7], item: 1 }, "").is_ok());
}

#[test]
fn agent_own_set() {
    // A labelled caller's agent runs in its own set: neither unlabelled nor another of its sets.
    let mut r = Rig::new();
    let (_, v) = r.login("alice", &[7], 11).unwrap();
    for labels in [vec![], vec![8]] {
        let agent = Content::Agent { labels, lease: SECOND };
        assert_eq!(refused(r.submit(v, agent, "")), Refusal::NotOwner);
    }
    assert!(r.records(|x| matches!(x, Record::Submitted { .. })).is_empty());
    assert!(r.submit(v, Content::Agent { labels: vec![7], lease: SECOND }, "").is_ok());
    // An unlabelled caller's names any set its principal owns, never the unlabelled one: that
    // agent is StartAgent's.
    let (_, u) = r.login("alice", &[], 11).unwrap();
    assert!(r.submit(u, Content::Agent { labels: vec![8], lease: SECOND }, "").is_ok());
    let unlabelled = Content::Agent { labels: vec![], lease: SECOND };
    assert_eq!(refused(r.submit(u, unlabelled, "")), Refusal::NotOwner);
}

#[test]
fn reaches_and_render() {
    let mut r = Rig::new();
    let (_, v) = r.login("alice", &[7], 11).unwrap();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    let hostile = "\u{1b}[2J\u{1b}]0;owned\u{7}\u{202e}evil\u{2066}\u{200b}\"\\";
    r.submit(u, Content::Note { what: hostile.into() }, hostile).unwrap();
    r.submit(v, Content::Note { what: "secret words".into() }, "secret reason").unwrap();
    r.open(1, "alice", 12);
    r.open(2, "bob", 22);
    let screens = r.pending(1);
    assert_eq!(screens.len(), 2);
    // The attack: a field full of ANSI escapes renders inert, printable ASCII only.
    for s in &screens {
        assert!(s.text.chars().all(|c| (' '..='~').contains(&c)), "{:?}", s.text);
        assert!(!s.text.contains("secret"), "a labelled request shows no free text: {:?}", s.text);
    }
    // Bob sees none of alice's.
    assert!(r.pending(2).is_empty());
    // The keeper: a channel that reaches every account's requests shows bob alice's labelled one.
    let mut r = Rig::with(Policy { reaches: |_, _| true, ..Policy::SHIPPED });
    let (_, v) = r.login("alice", &[7], 11).unwrap();
    r.submit(v, Content::Note { what: "x".into() }, "").unwrap();
    r.open(2, "bob", 22);
    assert_eq!(r.pending(2).len(), 1);
}

#[test]
fn rendered_here_and_hash_matches() {
    let mut r = Rig::new();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    let id = r.submit(u, Content::Note { what: "x".into() }, "").unwrap();
    r.open(1, "alice", 12);
    r.open(2, "alice", 12);
    r.open(3, "bob", 22);
    assert_eq!(refusal(r.approve(1, id)), Refusal::Unknown, "not yet rendered: frozen");
    r.pending(1);
    assert_eq!(refusal(r.approve(2, id)), Refusal::NotRendered, "rendered on another channel");
    assert_eq!(refusal(r.approve(3, id)), Refusal::Unknown, "another principal's request");
    let wrong = [9; 32];
    assert_eq!(
        refusal(r.call(EventKind::Approve { channel: 1, request: id, hash: wrong })),
        Refusal::HashMismatch
    );
    // The last channel to render it answers it.
    r.pending(2);
    assert_eq!(refusal(r.approve(1, id)), Refusal::NotRendered);
    assert_eq!(r.approve(2, id), Some(Answer::Ok));
    assert_eq!(r.records(|x| matches!(x, Record::Approved { .. })).len(), 1);
    assert_eq!(r.pending_in(&d(1001, &[])), 0);
    // Deny the same way.
    let id = r.submit(u, Content::Note { what: "y".into() }, "").unwrap();
    assert_eq!(refusal(r.call(EventKind::Deny { channel: 2, request: id })), Refusal::Unknown);
    r.pending(2);
    assert_eq!(r.call(EventKind::Deny { channel: 2, request: id }), Some(Answer::Ok));
    assert_eq!(r.records(|x| matches!(x, Record::Denied { .. })).len(), 1);
}

#[test]
fn a_closed_channels_id_reopened_answers_nothing_it_rendered() {
    let mut r = Rig::new();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    let id = r.submit(u, Content::Note { what: "x".into() }, "").unwrap();
    r.open(1, "alice", 12);
    r.pending(1);
    r.call(EventKind::ApprovalClosed { channel: 1 });
    // The attack: a new connection given the closed one's id approves what it never showed.
    r.open(1, "alice", 12);
    assert_eq!(refusal(r.approve(1, id)), Refusal::NotRendered);
    r.pending(1);
    assert_eq!(r.approve(1, id), Some(Answer::Ok));
}

#[test]
fn reaches_only_requests_whose_labels_the_principal_owns() {
    // Bob works under {7} but owns 9 only: a session there is refused at entry, so with that
    // check broken his {7} request exists, and his own channel still does not reach it.
    let mut r = Rig::with(Policy { owns_labels: |_| Ok(()), ..Policy::SHIPPED });
    let (_, v) = r.login("bob", &[7], 21).unwrap();
    let id = r.submit(v, Content::Note { what: "x".into() }, "").unwrap();
    r.open(1, "bob", 22);
    assert!(r.pending(1).is_empty());
    assert_eq!(refusal(r.approve(1, id)), Refusal::Unknown);
    // And a request asking for labels he lacks is refused at submission.
    let mut r = Rig::new();
    let (_, b) = r.login("bob", &[], 21).unwrap();
    let agent = Content::Agent { labels: vec![7], lease: SECOND };
    assert_eq!(refused(r.submit(b, agent, "")), Refusal::NotOwner);
}

#[test]
fn a_granted_lease_starts_in_its_own_domain() {
    let mut r = Rig::new();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    let id = r.submit(u, Content::Agent { labels: vec![7], lease: SECOND }, "").unwrap();
    // Recorded under the target's domain from submission on.
    assert_eq!(*r.records(|x| matches!(x, Record::Submitted { .. }))[0].domain(), d(1001, &[7]));
    r.open(1, "alice", 12);
    r.pending(1);
    assert_eq!(r.approve(1, id), Some(Answer::Ok));
    let vault = inspect::domain(&r.store, &d(1001, &[7])).unwrap();
    assert_eq!(vault.leases.len(), 1);
    assert!(
        vault.leases.values().all(|l| l.granted && l.state == redoubt_steward::gen::lease::State::Running)
    );
    // A grant whose start fails is audited, not answered.
    let id = r.submit(u, Content::Agent { labels: vec![7], lease: SECOND }, "").unwrap();
    r.pending(1);
    r.fail.push(Kind::Lease);
    r.approve(1, id);
    assert_eq!(r.records(|x| matches!(x, Record::StartFailed { .. })).len(), 1);
}

#[test]
fn not_locked_keeps_a_grant_pending() {
    let mut r = Rig::new();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    let id = r.submit(u, Content::Agent { labels: vec![7], lease: SECOND }, "").unwrap();
    r.open(1, "alice", 12);
    r.pending(1);
    for _ in 0..3 {
        r.blame(1001, &[7]);
    }
    assert_eq!(refusal(r.approve(1, id)), Refusal::LockedOut);
    assert_eq!(r.pending_in(&d(1001, &[])), 1);
    r.now += BLAME_WINDOW;
    assert_eq!(r.approve(1, id), Some(Answer::Ok));
}

#[test]
fn a_declassification_copies_out_exactly_its_snapshot() {
    let mut r = Rig::new();
    let (_, v) = r.login("alice", &[7], 11).unwrap();
    r.volumes.insert((vec![7], 1), b"the plan".to_vec());
    let id = r.submit(v, Content::Declassify { labels: vec![7], item: 1 }, "").unwrap();
    // Read through a reader budget carrying exactly the item's labels, destroyed after.
    assert_eq!(r.reads, [(vec![7], 1, Some(vec![7]))]);
    assert_eq!(r.budgets.values().filter(|b| b.1 == [7]).count(), 1, "only the session's is left");
    // The item changes after submission; what is copied out is the snapshot.
    r.volumes.insert((vec![7], 1), b"another plan".to_vec());
    r.open(1, "alice", 12);
    let screen = r.pending(1);
    assert!(screen[0].text.contains("the plan"), "{:?}", screen[0].text);
    assert_eq!(r.approve(1, id), Some(Answer::Ok));
    assert_eq!(r.volumes[&(vec![], 1)], b"the plan");
    assert_eq!(r.writes, [(vec![], 1, b"the plan".to_vec(), None)]);
    let rec = r.records(|x| matches!(x, Record::Declassified { .. }));
    assert_eq!(*rec[0].domain(), d(1001, &[7]), "stamped with the labelled side");
    let Record::Declassified { bytes, reader, .. } = rec[0].record() else { unreachable!() };
    assert_eq!((bytes.as_slice(), *reader > 1000), (&b"the plan"[..], true));
}

#[test]
fn a_push_writes_its_snapshot_through_a_writer_with_the_target_labels() {
    let mut r = Rig::new();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    r.volumes.insert((vec![], 4), b"input".to_vec());
    let id = r.submit(u, Content::Push { source: 4, target: vec![7], item: 9 }, "").unwrap();
    r.volumes.insert((vec![], 4), b"changed".to_vec());
    r.open(1, "alice", 12);
    let screen = r.pending(1);
    assert!(screen[0].text.contains("5 bytes, sha256"), "{:?}", screen[0].text);
    assert_eq!(r.approve(1, id), Some(Answer::Ok));
    assert_eq!(r.writes, [(vec![7], 9, b"input".to_vec(), Some(vec![7]))]);
    let rec = r.records(|x| matches!(x, Record::Pushed { .. }));
    assert_eq!(*rec[0].domain(), d(1001, &[7]));
    assert!(r.records(|x| matches!(x, Record::Approved { .. })).iter().all(|a| *a.domain() == d(1001, &[7])));
    // A failed write is audited, and its writer destroyed.
    let id = r.submit(u, Content::Push { source: 4, target: vec![7], item: 9 }, "").unwrap();
    r.pending(1);
    r.fail.push(Kind::Crossing);
    r.approve(1, id);
    assert_eq!(r.records(|x| matches!(x, Record::PushFailed { .. })).len(), 1);
}

#[test]
fn sponsor_session_and_notify_sponsor() {
    let mut r = Rig::new();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    let (_, v) = r.login("alice", &[7], 11).unwrap();
    let (_, other) = r.login("bob", &[], 21).unwrap();
    let (lease, ab) = r.agent(u, SECOND).unwrap();
    assert!(EventKind::EndLease { badge: v, lease }.ahead());
    assert_eq!(
        refusal(r.call(EventKind::EndLease { badge: v, lease })),
        Refusal::NotSponsor,
        "from a vault session"
    );
    assert_eq!(
        refusal(r.call(EventKind::EndLease { badge: other, lease })),
        Refusal::Unknown,
        "another account"
    );
    assert_eq!(
        refusal(r.call(EventKind::EndLease { badge: ab, lease })),
        Refusal::NotSponsor,
        "the agent itself"
    );
    let from = r.outputs.len();
    assert_eq!(r.call(EventKind::EndLease { badge: u, lease }), Some(Answer::Ok));
    assert_eq!(r.sessions_in(&d(1001, &[])), 1);
    assert_eq!(r.notices(from), [(Notified::Session(u), Notice::LeaseEnded { lease })]);
    assert_eq!(*r.records(|x| matches!(x, Record::LeaseEnded { .. }))[0].domain(), d(1001, &[]));
}

#[test]
fn a_lease_ends_at_its_deadline_or_its_process_exit() {
    let mut r = Rig::new();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    let (lease, _) = r.agent(u, SECOND).unwrap();
    let object = Object { domain: d(1001, &[]), kind: Kind::Lease, id: lease };
    let from = r.outputs.len();
    r.call(EventKind::Exited { object });
    assert_eq!(r.sessions_in(&d(1001, &[])), 1);
    assert_eq!(r.notices(from).len(), 1);
}

#[test]
fn notify_reaches_only_channels_with_the_requests_labels() {
    let mut r = Rig::new();
    let (_, u) = r.login("alice", &[], 11).unwrap();
    let (_, v7) = r.login("alice", &[7], 11).unwrap();
    let (_, v8) = r.login("alice", &[8], 11).unwrap();
    r.open(1, "alice", 12);
    let from = r.outputs.len();
    r.submit(v7, Content::Note { what: "x".into() }, "").unwrap();
    let to: Vec<Notified> = r.notices(from).into_iter().map(|n| n.0).collect();
    assert!(to.contains(&Notified::Session(v7)) && to.contains(&Notified::Channel(1)));
    assert!(!to.contains(&Notified::Session(u)) && !to.contains(&Notified::Session(v8)));
    let from = r.outputs.len();
    r.submit(u, Content::Note { what: "x".into() }, "").unwrap();
    assert_eq!(r.notices(from).len(), 4, "an unlabelled request may reach every channel");
}

#[test]
fn audit_visible() {
    let mut r = Rig::new();
    r.login("alice", &[7], 11).unwrap();
    r.login("alice", &[], 11).unwrap();
    let all = r.audit.clone();
    assert_eq!(inspect::audit_view(&r.store, &all, &Labels::empty()).len(), 1);
    assert_eq!(inspect::audit_view(&r.store, &all, &Labels::new(&[7]).unwrap()).len(), 2);
}

#[test]
fn a_failed_start_ends_the_session() {
    let mut r = Rig::new();
    r.fail.push(Kind::Session);
    assert_eq!(refused(r.login("alice", &[], 11)), Refusal::Failed);
    assert_eq!(r.sessions_in(&d(1001, &[])), 0);
    assert!(inspect::index(&r.store).ids.is_empty());
}

#[test]
fn an_excluded_event_makes_the_steward_exit() {
    let mut r = Rig::new();
    let (_, b) = r.login("alice", &[], 11).unwrap();
    let (l, _) = r.agent(b, SECOND).unwrap();
    let object = Object { domain: d(1001, &[]), kind: Kind::Lease, id: l };
    r.feed(0, EventKind::Done { object, result: Ok(vec![]) });
    assert!(r.exited && inspect::exited(&r.store));
    let e = decide(
        &mut r.store,
        Event { now: r.now, random: [1; RANDOM_WORDS], reply: 1, kind: EventKind::Pending { channel: 1 } },
    );
    assert!(e.exit && e.outputs.is_empty());
}

#[test]
fn an_event_naming_nothing_gets_one_answer() {
    let mut r = Rig::new();
    for kind in [
        EventKind::Submit { badge: 77, content: Content::Note { what: "x".into() }, reason: "".into() },
        EventKind::StartAgent { badge: 77, lease: SECOND },
        EventKind::EndSession { badge: 77 },
        EventKind::EndLease { badge: 77, lease: 1 },
        EventKind::Pending { channel: 5 },
        EventKind::ChannelClosed { session: 5 },
    ] {
        assert_eq!(refusal(r.call(kind)), Refusal::Unknown);
    }
}

#[test]
fn the_same_events_give_the_same_effects() {
    let run = || {
        let mut r = Rig::new();
        let (_, u) = r.login("alice", &[], 11).unwrap();
        r.submit(u, Content::Note { what: "x".into() }, "why").unwrap();
        r.outputs
    };
    assert_eq!(run(), run());
}
