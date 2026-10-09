//! The steward against a kernel that records its calls (servers/steward.md, "Principals",
//! "Fixed sub-budgets per label set", "Authentication and sessions" and "The steward's
//! protocol"): its start from the manifest lines, and each call through the protocol, the core
//! and the batches it names.

use redoubt_rt::abi::{BudgetSpec, Error, FOREVER, Handle, ReceivedHandles};
use redoubt_rt::ipc::Caller;
use redoubt_rt::wire::proto::steward::{
    ChannelClosed, EndSession, ErrorCode, Login, LoginReply, Message, Reply, StartAgent, Watch,
};
use redoubt_steward::domain::Domain;
use redoubt_steward::effect::Step;
use redoubt_steward::hash::key_id;
use redoubt_steward_server::drive::MINTED;
use redoubt_steward_server::protocol::{APPROVAL, INIT, SSHD, Serving, answer_with, watches};
use redoubt_steward_server::{Kernel, StartError, Steward, start};

const USERS: usize = 0;
const SERVERS: &str = "servers 6";
const SIZES: &str = "sizes session=10,2,5 agent=10,2,5 sub_agent=5,1,2 crossing=2,1,1 cost=1";
const ALICE_KEY: [u8; 32] = [1; 32];
/// The handle a login's console connection arrives as.
const CONSOLE: u32 = 9;
const BOB_KEY: [u8; 32] = [2; 32];

fn alice() -> String {
    format!(
        "principal \"alice\" account=1001 login=[{}] approval=[21] owned=[7] sets=[[],[7]] top=1000,30,300",
        key_id(&ALICE_KEY)
    )
}

fn bob() -> String {
    format!(
        "principal \"bob\" account=1002 login=[{}] approval=[22] owned=[] sets=[[]] top=500,10,100",
        key_id(&BOB_KEY)
    )
}

/// What the kernel was asked, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    Create {
        parent: usize,
        pages: u64,
        processes: u32,
        labels: Vec<u64>,
        account: u64,
    },
    Destroy(usize),
    Mint {
        badge: u64,
        stamp: usize,
    },
    Connect {
        labels: Vec<u64>,
        slot: u16,
    },
    Console(Option<u32>),
    Release(usize),
    Launch {
        budget: usize,
        connections: Vec<Option<usize>>,
        context: Option<String>,
    },
    /// A context's relay started in `budget`; its control connection is the handle made.
    LaunchRelay {
        budget: usize,
        control: usize,
    },
    /// The login's console given to the relay with a note; the console kept is the handle made.
    Attach {
        relay: usize,
        note: String,
        kept: usize,
    },
    Detach {
        relay: usize,
        note: String,
    },
}

/// Budget and handle `i` are the `i`th the kernel made; `USERS` is the one the steward holds.
#[derive(Default)]
struct Recorder {
    calls: Vec<Call>,
    made: usize,
    words: u64,
    /// Fail the `n`th kernel call that makes something.
    fail_at: Option<usize>,
    users_busy: bool,
    /// What every relay's `detach` answers, once recorded: `Ok` unless a test says.
    detach: Option<Error>,
}

impl Recorder {
    fn make(&mut self) -> Result<usize, Error> {
        if self.fail_at == Some(self.made) {
            return Err(Error::OutOfMemory);
        }
        self.made += 1;
        Ok(self.made)
    }

    fn creates(&self) -> Vec<&Call> {
        self.calls.iter().filter(|c| matches!(c, Call::Create { .. })).collect()
    }
}

impl Kernel for Recorder {
    type Budget = usize;
    type Handle = usize;

    fn create(&mut self, parent: usize, spec: &BudgetSpec) -> Result<usize, Error> {
        assert_eq!(spec.deadline, FOREVER);
        let made = self.make()?;
        self.calls.push(Call::Create {
            parent,
            pages: spec.pages,
            processes: spec.processes,
            labels: spec.labels.as_slice().to_vec(),
            account: spec.account,
        });
        Ok(made)
    }

    fn empty(&mut self, _budget: usize) -> Result<bool, Error> { Ok(!self.users_busy) }

    fn destroy(&mut self, budget: usize) -> Result<(), Error> {
        self.calls.push(Call::Destroy(budget));
        Ok(())
    }

    fn budget_id(&self, budget: usize) -> u64 { budget as u64 }

    fn mint(&mut self, badge: u64, stamp: usize) -> Result<usize, Error> {
        let made = self.make()?;
        self.calls.push(Call::Mint { badge, stamp });
        Ok(made)
    }

    /// The binding table's shape: slot 2 (the vault) binds to nothing unlabelled, slot 3 (the
    /// network) to nothing labelled.
    fn connect(&mut self, domain: &Domain, slot: u16) -> Result<Option<usize>, Error> {
        let labels = domain.labels().as_slice().to_vec();
        let binds = redoubt_steward_server::own::binds(!labels.is_empty(), slot);
        self.calls.push(Call::Connect { labels, slot });
        if binds { self.make().map(Some) } else { Ok(None) }
    }

    fn console(&mut self, handle: Option<Handle>, relay: Option<Handle>) {
        assert_eq!(handle.is_some(), relay.is_some(), "a login brings both consoles or none");
        self.calls.push(Call::Console(handle.map(|h| h.index())));
    }

    fn release(&mut self, handle: usize) { self.calls.push(Call::Release(handle)); }

    fn launch(
        &mut self,
        _domain: &Domain,
        budget: usize,
        connections: &[Option<usize>],
        context: Option<&str>,
    ) -> Result<u64, Error> {
        let pid = self.make()? as u64;
        let context = context.map(String::from);
        self.calls.push(Call::Launch { budget, connections: connections.to_vec(), context });
        Ok(pid)
    }

    fn launch_relay(&mut self, _domain: &Domain, budget: usize) -> Result<(u64, usize), Error> {
        let control = self.make()?;
        self.calls.push(Call::LaunchRelay { budget, control });
        Ok((control as u64, control))
    }

    fn attach(&mut self, relay: usize, note: &str) -> Result<usize, Error> {
        let kept = self.make()?;
        self.calls.push(Call::Attach { relay, note: note.into(), kept });
        Ok(kept)
    }

    fn detach(&mut self, relay: usize, note: &str) -> Result<(), Error> {
        self.calls.push(Call::Detach { relay, note: note.into() });
        self.detach.map_or(Ok(()), Err)
    }

    fn random(&mut self) -> Result<u64, Error> {
        self.words += 1;
        Ok(self.words * 0x1_0000)
    }

    fn now(&mut self) -> u64 { self.words }
}

fn shape(k: &Recorder) -> Vec<(usize, u64, u32, Vec<u64>, u64)> {
    k.creates()
        .into_iter()
        .map(|c| match c {
            Call::Create { parent, pages, processes, labels, account } => {
                (*parent, *pages, *processes, labels.clone(), *account)
            }
            _ => unreachable!(),
        })
        .collect()
}

fn lines(extra: &[&str]) -> Vec<String> {
    let mut l = vec![alice(), bob(), "keyd [31]".into(), SERVERS.into(), SIZES.into()];
    l.extend(extra.iter().map(|s| s.to_string()));
    l
}

fn started(k: &mut Recorder) -> Steward<usize, usize> {
    let l = lines(&["label \"alice-secrets\" id=7"]);
    let l: Vec<&str> = l.iter().map(String::as_str).collect();
    let s = start(&l, USERS, k).unwrap();
    k.calls.clear();
    s
}

// ---- the start ----

#[test]
fn each_principal_gets_a_top_budget_under_users_and_a_sub_budget_per_label_set() {
    let mut k = Recorder::default();
    let l = lines(&[]);
    let l: Vec<&str> = l.iter().map(String::as_str).collect();
    let s = start(&l, USERS, &mut k).unwrap();
    // The account on the top budget only, which the kernel stamps on all below it; each
    // sub-budget an equal share of the top, less its own object's page.
    assert_eq!(
        shape(&k),
        [
            (USERS, 1000, 30, vec![], 1001),
            (1, 499, 15, vec![], 0),
            (1, 499, 15, vec![7], 0),
            (USERS, 500, 10, vec![], 1002),
            (4, 499, 10, vec![], 0),
        ]
    );
    let names: Vec<(&str, u64, usize, usize)> =
        s.carved.iter().map(|c| (c.name.as_str(), c.account, c.top, c.subs.len())).collect();
    assert_eq!(names, [("alice", 1001, 1, 2), ("bob", 1002, 4, 1)]);
    let alice_vault = &s.carved[0].subs[1].0;
    assert_eq!(s.sub(alice_vault), Some(3));
}

#[test]
fn a_malformed_line_is_a_start_failure_before_any_carve() {
    for bad in ["servers 4 5", "label alice-secrets id=7", "home \"alice\" handle=walfsd:data path=home"] {
        let mut k = Recorder::default();
        let l = [alice(), bad.into(), SERVERS.into(), SIZES.into()];
        let l: Vec<&str> = l.iter().map(String::as_str).collect();
        let e = start(&l, USERS, &mut k).err().unwrap();
        assert!(matches!(&e, StartError::Lines(why) if why.starts_with("line 2: ")), "{bad}: {e:?}");
        assert!(k.calls.is_empty());
    }
}

#[test]
fn a_console_naming_no_principal_is_a_start_failure_naming_its_line() {
    let mut k = Recorder::default();
    let l = lines(&["console \"carol\""]);
    let l: Vec<&str> = l.iter().map(String::as_str).collect();
    assert_eq!(
        start(&l, USERS, &mut k).err().unwrap(),
        StartError::Lines("line 6: carol is no principal".into())
    );
    assert!(k.calls.is_empty());
}

#[test]
fn a_servers_count_other_than_the_binding_table_s_is_a_start_failure() {
    for n in [0, 5, 7] {
        let mut k = Recorder::default();
        let l = [alice(), format!("servers {n}"), SIZES.into()];
        let l: Vec<&str> = l.iter().map(String::as_str).collect();
        assert_eq!(start(&l, USERS, &mut k).err().unwrap(), StartError::Slots(n));
        assert!(k.calls.is_empty());
    }
}

#[test]
fn a_manifest_the_core_refuses_is_a_start_failure_before_any_carve() {
    let mut k = Recorder::default();
    // Bob's login key is alice's approval key: one key in two roles.
    let bob = bob().replace(&format!("login=[{}]", key_id(&BOB_KEY)), "login=[21]");
    let l = [alice(), bob, SERVERS.into(), SIZES.into()];
    let l: Vec<&str> = l.iter().map(String::as_str).collect();
    assert_eq!(start(&l, USERS, &mut k).err().unwrap(), StartError::Manifest);
    assert!(k.calls.is_empty());
}

/// A steward restarted beside the carves of an earlier one exits rather than carve a second set
/// (servers/init.md, "Restarts and reboots": `init` recreates `users` first).
#[test]
fn users_not_empty_is_a_start_failure_before_any_carve() {
    let mut k = Recorder { users_busy: true, ..Recorder::default() };
    let l = lines(&[]);
    let l: Vec<&str> = l.iter().map(String::as_str).collect();
    assert_eq!(start(&l, USERS, &mut k).err().unwrap(), StartError::UsersNotEmpty);
    assert!(k.calls.is_empty());
}

#[test]
fn limits_the_kernel_cannot_take_are_a_start_failure() {
    let mut k = Recorder::default();
    let alice = alice().replace("top=1000,30,300", "top=1000,30,4294967296");
    let l = [alice, SERVERS.into(), SIZES.into()];
    let l: Vec<&str> = l.iter().map(String::as_str).collect();
    assert_eq!(start(&l, USERS, &mut k).err().unwrap(), StartError::Limits { account: 1001 });
}

#[test]
fn a_refused_carve_is_a_start_failure_naming_the_budget() {
    for (at, labels) in [(0, vec![]), (2, vec![7])] {
        let mut k = Recorder { fail_at: Some(at), ..Recorder::default() };
        let l = lines(&[]);
        let l: Vec<&str> = l.iter().map(String::as_str).collect();
        let e = start(&l, USERS, &mut k).err().unwrap();
        assert_eq!(e, StartError::Carve { account: 1001, labels, error: Error::OutOfMemory });
    }
}

// ---- calls through the protocol ----

fn call(
    s: &mut Steward<usize, usize>,
    k: &mut Recorder,
    badge: u64,
    m: Message<'_>,
) -> Result<Reply<'static>, ErrorCode> {
    // An inline message travels in its words alone: one that came with a lend is malformed.
    let inline = matches!(
        m,
        Message::ChannelClosed(_)
            | Message::ApprovalClosed(_)
            | Message::EndLease(_)
            | Message::EndSession(_)
            | Message::Watch(_)
    );
    let mut buf = if inline { Vec::new() } else { vec![0u8; 4096] };
    let words = m.encode(&mut buf).unwrap();
    let opcode = words[0] as u32;
    let caller = Caller { badge, account: 0, labels: Default::default() };
    // A login brings the channel's two console connections: here handles 9 and 10.
    let mut handles = ReceivedHandles::new();
    if matches!(m, Message::Login(_)) {
        handles.push(Handle::new(CONSOLE)).unwrap();
        handles.push(Handle::new(CONSOLE + 1)).unwrap();
    }
    let mut serving = Serving::new(s, k);
    let outcome = answer_with(&mut serving, &caller, &words, &handles, &mut buf);
    assert!(!serving.exited, "the core exited");
    let reply = Reply::decode(opcode, &outcome.words, &buf, 0).unwrap();
    // Owned copies of what the tests read: a login's session and labels.
    reply.map(|r| match r {
        Reply::Login(l) => Reply::Login(LoginReply {
            session: l.session,
            name: Box::leak(l.name.to_string().into_boxed_str()),
            labels: Box::leak(l.labels.to_vec().into_boxed_slice()),
        }),
        Reply::ChannelClosed(_) => {
            Reply::ChannelClosed(redoubt_rt::wire::proto::steward::ChannelClosedReply {})
        }
        Reply::EndSession(_) => Reply::EndSession(redoubt_rt::wire::proto::steward::EndSessionReply {}),
        _ => panic!("a reply these tests do not read"),
    })
}

fn login<'a>(principal: &'a str, label: &'a str, key: &'a [u8]) -> Message<'a> {
    Message::Login(Login { principal, label, context: "", from: "", key })
}

/// A login is the core's session batch, run in order: the session's budget from its domain's
/// sub-budget, a zero-limit scope inside it, the context's console relay in the session's budget
/// and the login's console attached to it, the steward's badge on its own endpoint stamped with
/// the scope (in the minted range, never a root badge), a connection per shared slot (the
/// vault's bound to nothing for an unlabelled session), and the launch with them all; then the
/// reply carries the session's id and its labels.
#[test]
fn a_login_runs_the_session_batch_and_answers_the_session() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let Ok(Reply::Login(r)) = call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)) else { panic!() };
    assert!(r.session >= MINTED && r.labels.is_empty());
    let alice_unlabelled = 2;
    let session_budget = 6;
    let scope = 7;
    let relay = 8;
    let badge = match &k.calls[5] {
        Call::Mint { badge, stamp } => {
            assert_eq!(*stamp, scope);
            *badge
        }
        other => panic!("{other:?}"),
    };
    assert!(badge >= MINTED);
    let connects: Vec<Call> = (0..6).map(|slot| Call::Connect { labels: vec![], slot }).collect();
    assert_eq!(
        k.calls[..3],
        [
            Call::Console(Some(CONSOLE)),
            Call::Create { parent: alice_unlabelled, pages: 10, processes: 2, labels: vec![], account: 0 },
            Call::Create { parent: session_budget, pages: 0, processes: 0, labels: vec![], account: 0 },
        ]
    );
    // A new context's first attach has no note.
    assert_eq!(
        k.calls[3..5],
        [
            Call::LaunchRelay { budget: session_budget, control: relay },
            Call::Attach { relay, note: String::new(), kept: 9 },
        ]
    );
    assert_eq!(&k.calls[6..12], &connects[..]);
    match &k.calls[12] {
        Call::Launch { budget, connections, context } => {
            assert_eq!(*budget, session_budget);
            assert_eq!(context.as_deref(), Some(""), "the principal's default context");
            assert_eq!(connections.len(), 7, "the steward's, then six slots");
            assert_eq!(connections[3], None, "slot 2, the vault, binds to nothing unlabelled");
            assert!(connections.iter().enumerate().all(|(i, c)| i == 3 || c.is_some()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_vault_login_carves_from_the_vault_s_sub_budget_and_has_no_network() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let Ok(Reply::Login(r)) = call(&mut s, &mut k, SSHD, login("alice", "alice-secrets", &ALICE_KEY)) else {
        panic!()
    };
    assert_eq!(r.labels, 7u64.to_le_bytes());
    let alice_vault = 3;
    assert!(
        matches!(&k.calls[1], Call::Create { parent, labels, .. } if *parent == alice_vault && labels == &[7])
    );
    // The kernel refuses a budget under a labelled one without its labels: the session's scope,
    // under its budget (the sixth made), carries them too.
    let scope = k.creates().into_iter().find(|c| matches!(c, Call::Create { parent: 6, .. })).cloned();
    assert!(matches!(&scope, Some(Call::Create { pages: 0, labels, .. }) if labels == &[7]), "{scope:?}");
    let launch = k.calls.iter().find_map(|c| match c {
        Call::Launch { connections, .. } => Some(connections.clone()),
        _ => None,
    });
    let connections = launch.unwrap();
    assert_eq!(connections[4], None, "slot 3, the network, binds to nothing labelled");
    assert!(connections[3].is_some(), "slot 2, the vault, is bound");
}

/// The steward's refusals of a login, alike whatever is wrong (servers/steward.md, "Contexts"): a
/// key the manifest lists for bob used as alice, a label the principal does not own, a label no
/// manifest names, a principal it does not name, a context that is not a name, and a key that is
/// not 32 bytes; none carves anything.
#[test]
fn a_refused_login_makes_nothing() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let cases: [(&str, &str, &str, &[u8]); 10] = [
        ("alice", "", "", &BOB_KEY),
        ("bob", "alice-secrets", "", &BOB_KEY),
        ("alice", "carol-secrets", "", &ALICE_KEY),
        ("alice", "not a name!", "", &ALICE_KEY),
        ("carol", "", "", &ALICE_KEY),
        ("", "", "", &ALICE_KEY),
        ("alice", "", "Work", &ALICE_KEY),
        ("alice", "", "a.b", &ALICE_KEY),
        ("alice", "", "a:b", &ALICE_KEY),
        ("alice", "", "", &ALICE_KEY[..31]),
    ];
    for (principal, label, context, key) in cases {
        let m = Message::Login(Login { principal, label, context, from: "", key });
        assert_eq!(
            call(&mut s, &mut k, SSHD, m).unwrap_err(),
            ErrorCode::BadKey,
            "{principal} {label} {context}"
        );
    }
    assert!(k.creates().is_empty());
}

fn work(from: &str) -> Message<'_> {
    Message::Login(Login { principal: "alice", label: "", context: "work", from, key: &ALICE_KEY })
}

/// R80 through the server: a second login of an attached context takes it over and makes
/// nothing. The old channel is told who took it and let go, its console given back, which tells
/// `sshd` its channel is over; the new one is told where it was taken from; the reply names a
/// new attachment, and a close of the old one changes nothing.
#[test]
fn a_login_to_an_attached_context_takes_it_over() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let Ok(Reply::Login(first)) = call(&mut s, &mut k, SSHD, work("198.51.100.2:51234")) else { panic!() };
    let (relay, kept) = (8, 9);
    k.calls.clear();
    let Ok(Reply::Login(second)) = call(&mut s, &mut k, SSHD, work("203.0.113.7:40022")) else { panic!() };
    assert_ne!(second.session, first.session);
    assert!(k.creates().is_empty());
    assert_eq!(
        k.calls,
        [
            Call::Console(Some(CONSOLE)),
            Call::Detach {
                relay,
                note: "[context work taken over from 203.0.113.7:40022 at up 0h00m]\r\n".into()
            },
            Call::Release(kept),
            Call::Attach {
                relay,
                note: "[context work: reattached; taken over from 198.51.100.2:51234]\r\n".into(),
                kept: 17
            },
        ]
    );
    k.calls.clear();
    let stale = call(&mut s, &mut k, SSHD, Message::ChannelClosed(ChannelClosed { session: first.session }));
    assert_eq!(stale.unwrap_err(), ErrorCode::Unknown);
    assert!(k.calls.is_empty(), "the old channel's close changes nothing: {:?}", k.calls);
    // Another context of the same principal is its own.
    call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)).unwrap();
    assert_eq!(k.creates().len(), 2);
}

/// A takeover from a channel that has stopped reading: the relay's writer is stuck on the old
/// channel, so its note is never written and the detach runs past its bound. That is still a
/// detach: the old console is given back, the new channel attached, and the context runs on.
/// Any other failure of the detach ends the context, as a failed step does.
#[test]
fn a_takeover_from_a_stalled_channel_still_takes_it_over() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let Ok(Reply::Login(first)) = call(&mut s, &mut k, SSHD, work("198.51.100.2:51234")) else { panic!() };
    let (relay, kept, pid) = (8, 9, k.made as u64);
    k.calls.clear();
    k.detach = Some(Error::Timeout);
    let Ok(Reply::Login(second)) = call(&mut s, &mut k, SSHD, work("203.0.113.7:40022")) else { panic!() };
    assert_ne!(second.session, first.session);
    assert!(k.calls.contains(&Call::Release(kept)), "{:?}", k.calls);
    assert!(matches!(k.calls.last(), Some(Call::Attach { relay: r, .. }) if *r == relay), "{:?}", k.calls);
    assert!(!k.calls.iter().any(|c| matches!(c, Call::Destroy(_))), "{:?}", k.calls);
    assert!(s.exited(pid).is_some(), "the VM runs on");
    // A relay that refuses the detach outright is broken: the context ends.
    k.calls.clear();
    k.detach = Some(Error::Refused);
    assert!(call(&mut s, &mut k, SSHD, work("203.0.113.9:40023")).is_err());
    assert!(k.calls.iter().any(|c| matches!(c, Call::Destroy(_))), "{:?}", k.calls);
}

/// A session's launch is told its context, for its arguments (servers/steward.md,
/// "Authentication and sessions").
#[test]
fn a_sessions_launch_is_told_its_context() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let work =
        Message::Login(Login { principal: "alice", label: "", context: "work", from: "", key: &ALICE_KEY });
    call(&mut s, &mut k, SSHD, work).unwrap();
    let contexts: Vec<Option<String>> = k
        .calls
        .iter()
        .filter_map(|c| match c {
            Call::Launch { context, .. } => Some(context.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(contexts.last(), Some(&Some("work".to_string())), "{contexts:?}");
}

/// Each operation only on its badge class: on any other it is malformed, as an unknown opcode
/// is, so a session cannot log in and `sshd` cannot end a session by badge.
#[test]
fn every_operation_on_another_badge_class_is_malformed() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let session = MINTED | 5;
    for badge in [session, APPROVAL, INIT, 0, 4, MINTED - 1] {
        let e = call(&mut s, &mut k, badge, login("alice", "", &ALICE_KEY)).unwrap_err();
        assert_eq!(e, ErrorCode::Malformed, "login on {badge:#x}");
    }
    for badge in [SSHD, APPROVAL, INIT, 0] {
        let e = call(&mut s, &mut k, badge, Message::EndSession(EndSession {})).unwrap_err();
        assert_eq!(e, ErrorCode::Malformed, "end_session on {badge:#x}");
    }
    assert!(k.creates().is_empty());
    // On its own class, an operation the server binds no batch for yet is the core's unknown.
    let e = call(&mut s, &mut k, session, Message::StartAgent(StartAgent { lease: 1 })).unwrap_err();
    assert_eq!(e, ErrorCode::Unknown);
}

/// `watch` is `sshd`'s alone: the program holds it only from `sshd`'s root badge
/// (`protocol::watches`), and on any other badge it is malformed, so nobody else can learn of
/// the steward's end through it, nor hold its calls open.
#[test]
fn watch_is_held_only_from_sshd_and_malformed_on_any_other_badge() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let watch = Message::Watch(Watch {});
    let words = watch.encode(&mut []).unwrap();
    let caller = |badge| Caller { badge, account: 0, labels: Default::default() };
    assert!(watches(&caller(SSHD), &words));
    for badge in [MINTED | 5, APPROVAL, INIT, 0, 4, MINTED - 1] {
        assert!(!watches(&caller(badge), &words), "held on {badge:#x}");
        let e = call(&mut s, &mut k, badge, Message::Watch(Watch {})).unwrap_err();
        assert_eq!(e, ErrorCode::Malformed, "watch on {badge:#x}");
    }
    // Another message on `sshd`'s badge is not a watch.
    let closed = Message::ChannelClosed(ChannelClosed { session: 1 }).encode(&mut []).unwrap();
    assert!(!watches(&caller(SSHD), &closed));
    assert!(k.creates().is_empty());
}

/// The batch stops at the first step that fails: the launch refused, the login is refused as
/// failed, and the session's budget is destroyed; no process is routed. The server keeps the
/// failed step, with the kernel's error, to say.
#[test]
fn a_failed_step_stops_the_batch_and_destroys_what_it_made() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    // Made so far by the start: 5 budgets. The session's budget, scope, relay, console kept,
    // badge and five connections are the next 10; the launch is the eleventh.
    k.fail_at = Some(5 + 10);
    assert_eq!(call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)).unwrap_err(), ErrorCode::Failed);
    assert!(!k.calls.iter().any(|c| matches!(c, Call::Launch { .. })));
    assert!(k.calls.contains(&Call::Destroy(6)), "{:?}", k.calls);
    assert!(matches!(s.failed.as_slice(), [(Step::Launch { .. }, _)]), "{:?}", s.failed);
}

/// The channel's close detaches the context (R80): the relay lets the channel go and its console
/// is given back, nothing is destroyed, and the VM runs on. The next login reattaches, told so.
#[test]
fn closing_the_channel_detaches_the_context_and_a_login_reattaches() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let Ok(Reply::Login(r)) = call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)) else { panic!() };
    let pid = k.made as u64;
    let (relay, kept) = (8, 9);
    k.calls.clear();
    let closed = call(&mut s, &mut k, SSHD, Message::ChannelClosed(ChannelClosed { session: r.session }));
    assert!(closed.is_ok(), "{closed:?} {:?}", k.calls);
    assert_eq!(k.calls, [Call::Detach { relay, note: String::new() }, Call::Release(kept)]);
    assert!(s.exited(pid).is_some(), "the VM runs on");
    // Again: the attachment is gone.
    let again = call(&mut s, &mut k, SSHD, Message::ChannelClosed(ChannelClosed { session: r.session }));
    assert_eq!(again.unwrap_err(), ErrorCode::Unknown);
    k.calls.clear();
    let Ok(Reply::Login(back)) = call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)) else { panic!() };
    assert_ne!(back.session, r.session);
    assert_eq!(
        k.calls,
        [
            Call::Console(Some(CONSOLE)),
            Call::Attach { relay, note: "[context default: reattached]\r\n".into(), kept: 17 },
        ]
    );
}

/// `sshd`'s end takes every channel with it: each attached context is detached and runs on
/// (servers/steward.md, R80), and the next login reattaches.
#[test]
fn sshd_gone_detaches_every_attached_context() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)).unwrap();
    let pid = k.made as u64;
    k.calls.clear();
    s.event(&mut k, redoubt_steward::event::EventKind::SshdGone, 0).unwrap();
    assert_eq!(k.calls, [Call::Detach { relay: 8, note: String::new() }, Call::Release(9)]);
    assert!(s.exited(pid).is_some());
    k.calls.clear();
    s.event(&mut k, redoubt_steward::event::EventKind::SshdGone, 0).unwrap();
    assert!(k.calls.is_empty(), "nothing is attached");
    call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)).unwrap();
    assert!(k.calls.iter().any(|c| matches!(c, Call::Attach { .. })));
}

/// A VM that dies ends its session: the exit is the core's `Exited`, which destroys the budget.
#[test]
fn a_dead_vm_ends_its_session() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let Ok(Reply::Login(r)) = call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)) else { panic!() };
    let pid = k.made as u64;
    let object = s.exited(pid).unwrap();
    assert_eq!(object.id, r.session);
    k.calls.clear();
    s.event(&mut k, redoubt_steward::event::EventKind::Exited { object }, 0).unwrap();
    assert!(k.calls.iter().any(|c| matches!(c, Call::Destroy(_))));
    assert!(s.exited(pid).is_none());
}

/// The console principal's session (servers/steward.md, "Authentication and sessions"): opened
/// at the start with no key in the principal's unlabelled domain, and opened again when its VM
/// ends; without a `console` line there is none.
#[test]
fn the_console_session_opens_at_the_start_and_again_when_it_ends() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    assert!(s.open_console(&mut k).unwrap().is_empty());
    assert!(k.calls.is_empty(), "no console line, no console session");
    let l = lines(&["label \"alice-secrets\" id=7", "console \"alice\""]);
    let l: Vec<&str> = l.iter().map(String::as_str).collect();
    let mut k = Recorder::default();
    let mut s = start(&l, USERS, &mut k).unwrap();
    k.calls.clear();
    s.open_console(&mut k).unwrap();
    let alice_unlabelled = 2;
    assert!(
        matches!(&k.calls[0], Call::Create { parent, labels, .. } if *parent == alice_unlabelled && labels.is_empty())
    );
    let launches = |k: &Recorder| k.calls.iter().filter(|c| matches!(c, Call::Launch { .. })).count();
    assert_eq!(launches(&k), 1);
    let pid = k.made as u64;
    let object = s.exited(pid).unwrap();
    s.event(&mut k, redoubt_steward::event::EventKind::Exited { object }, 0).unwrap();
    assert_eq!(launches(&k), 2, "reopened");
    assert!(s.exited(pid).is_none() && s.exited(k.made as u64).is_some());
}

/// From `init`'s writer to the steward's parser over the image's own manifest: every line `init`
/// hands the steward reads, the steward starts on them, and its own lines say what the manifest
/// does (servers/steward.md, "The manifest lines").
#[test]
fn the_image_s_lines_round_trip_from_init_to_the_steward() {
    let image = include_str!("../../../image/manifest.json");
    let m = redoubt_init::read(image.as_bytes(), redoubt_init::ARENA_PAGES).unwrap();
    let entry = m.servers.iter().find(|s| s.name == "steward").unwrap();
    // The bundle key reaches only a signed volume's server and the entries' lengths only bootfsd;
    // the steward's lines use neither.
    let args = redoubt_init::check::args(&m, entry, &[0; 32], &[]);
    let lines: Vec<&str> = args.iter().map(String::as_str).filter(|a| !a.starts_with("buckets=")).collect();
    let mut k = Recorder::default();
    let s = start(&lines, USERS, &mut k).unwrap();
    let names: Vec<&str> = s.carved.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["alice", "bob"]);
    assert_eq!(s.own.labels, [("alice-secrets".to_string(), 7)]);
    assert_eq!(s.own.console.as_deref(), Some("alice"));
    assert_eq!(s.own.net("alice").unwrap().rules, ["0.0.0.0/0:80,443"]);
    assert!(s.label("alice-secrets").is_some());
}
