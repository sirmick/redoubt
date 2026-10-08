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
    Create { parent: usize, pages: u64, processes: u32, labels: Vec<u64>, account: u64 },
    Destroy(usize),
    Mint { badge: u64, stamp: usize },
    Connect { labels: Vec<u64>, slot: u16 },
    Console(Option<u32>),
    Release(usize),
    Launch { budget: usize, connections: Vec<Option<usize>> },
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

    fn console(&mut self, handle: Option<Handle>) {
        self.calls.push(Call::Console(handle.map(|h| h.index())));
    }

    fn release(&mut self, handle: usize) { self.calls.push(Call::Release(handle)); }

    fn launch(
        &mut self,
        _domain: &Domain,
        budget: usize,
        connections: &[Option<usize>],
    ) -> Result<u64, Error> {
        let pid = self.make()? as u64;
        self.calls.push(Call::Launch { budget, connections: connections.to_vec() });
        Ok(pid)
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
    // A login brings the channel's console connection: here handle 9.
    let mut handles = ReceivedHandles::new();
    if matches!(m, Message::Login(_)) {
        handles.push(Handle::new(CONSOLE)).unwrap();
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
    Message::Login(Login { principal, label, key })
}

/// A login is the core's session batch, run in order: the session's budget from its domain's
/// sub-budget, a zero-limit scope inside it, the steward's badge on its own endpoint stamped
/// with the scope (in the minted range, never a root badge), a connection per shared slot (the
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
    let badge = match &k.calls[3] {
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
    assert_eq!(&k.calls[4..10], &connects[..]);
    match &k.calls[10] {
        Call::Launch { budget, connections } => {
            assert_eq!(*budget, session_budget);
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

/// The steward's typed refusals: a key the manifest lists for bob used as alice, a label the
/// principal does not own, a label no manifest names, and a key that is not 32 bytes; none
/// carves anything.
#[test]
fn a_refused_login_makes_nothing() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let cases: [(&str, &str, &[u8], ErrorCode); 5] = [
        ("alice", "", &BOB_KEY, ErrorCode::BadKey),
        ("bob", "alice-secrets", &BOB_KEY, ErrorCode::NotOwner),
        ("alice", "carol-secrets", &ALICE_KEY, ErrorCode::NotOwner),
        ("alice", "not a name!", &ALICE_KEY, ErrorCode::NotOwner),
        ("alice", "", &ALICE_KEY[..31], ErrorCode::BadKey),
    ];
    for (principal, label, key, want) in cases {
        assert_eq!(
            call(&mut s, &mut k, SSHD, login(principal, label, key)).unwrap_err(),
            want,
            "{principal} {label}"
        );
    }
    assert!(k.creates().is_empty());
    assert_eq!(call(&mut s, &mut k, SSHD, login("carol", "", &ALICE_KEY)).unwrap_err(), ErrorCode::Unknown);
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
    // Made so far by the start: 5 budgets. The session's budget, scope, badge and five
    // connections are the next 8; the launch is the ninth.
    k.fail_at = Some(5 + 8);
    assert_eq!(call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)).unwrap_err(), ErrorCode::Failed);
    assert!(!k.calls.iter().any(|c| matches!(c, Call::Launch { .. })));
    assert!(k.calls.contains(&Call::Destroy(6)), "{:?}", k.calls);
    assert!(matches!(s.failed.as_slice(), [(Step::Launch { .. }, _)]), "{:?}", s.failed);
}

/// The channel's close ends the session: its process's route goes before its budget does, so the
/// exit notice that follows is late and dropped, and its connections are given back.
#[test]
fn closing_the_channel_destroys_the_session_and_its_exit_is_late() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let Ok(Reply::Login(r)) = call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)) else { panic!() };
    let pid = k.made as u64;
    assert!(s.exited(pid).is_some());
    k.calls.clear();
    let closed = call(&mut s, &mut k, SSHD, Message::ChannelClosed(ChannelClosed { session: r.session }));
    assert!(closed.is_ok(), "{closed:?} {:?}", k.calls);
    assert!(s.exited(pid).is_none());
    assert!(k.calls.iter().any(|c| matches!(c, Call::Destroy(_))));
    assert_eq!(k.calls.iter().filter(|c| matches!(c, Call::Release(_))).count(), 6, "{:?}", k.calls);
    // Again: the session is gone.
    let again = call(&mut s, &mut k, SSHD, Message::ChannelClosed(ChannelClosed { session: r.session }));
    assert_eq!(again.unwrap_err(), ErrorCode::Unknown);
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
