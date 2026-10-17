//! The steward against a kernel that records its calls (servers/steward.md, "Principals",
//! "Fixed sub-budgets per label set", "Authentication and sessions" and "The steward's
//! protocol"): its start from the manifest lines, and each call through the protocol, the core
//! and the batches it names.

use std::collections::BTreeMap;

use redoubt_rt::abi::{BudgetSpec, Error, FOREVER, Handle, ReceivedHandles};
use redoubt_rt::ipc::Caller;
use redoubt_rt::wire::proto::steward::{
    ApprovalClosed, ApprovalClosedReply, ApprovalOpened, ApprovalOpenedReply, Approve, ApproveReply, Blame,
    ChannelClosed, Contexts, Deny, DenyReply, Detach, EndContext, EndLease, EndSession, ErrorCode,
    LeaseEnded, Login, LoginReply, Message, Pending, PendingReply, Rebind, RebindReply, Reconnected,
    ReconnectedReply, Reply, ServerRestarted, StartAgent, Submit, SubmitReply, Watch,
};
use redoubt_steward::audit::Record;
use redoubt_steward::consts::{BLAME_COUNT, BLAME_WINDOW, MAX_LEASE};
use redoubt_steward::domain::Domain;
use redoubt_steward::effect::{Kind, Object, Output, Step};
use redoubt_steward::event::EventKind;
use redoubt_steward::hash::key_id;
use redoubt_steward_server::cleanup::{RETRY_US, TRIES};
use redoubt_steward_server::drive::MINTED;
use redoubt_steward_server::own::{Child, binds};
use redoubt_steward_server::protocol::{APPROVAL, INIT, SSHD, Serving, answer_with, watches};
use redoubt_steward_server::{Kernel, SLOTS, StartError, Steward, start};

const USERS: usize = 0;
const SERVERS: &str = "servers 6";
const SIZES: &str = "sizes session=10,2,5 agent=10,2,5 sub_agent=5,1,2 crossing=2,1,1 cost=1";
const ALICE_KEY: [u8; 32] = [1; 32];
/// Alice's approval key, which opens her `approve@box` channel and nothing else.
const ALICE_APPROVAL_KEY: [u8; 32] = [4; 32];
/// The handle a login's console connection arrives as.
const CONSOLE: u32 = 9;
const BOB_KEY: [u8; 32] = [2; 32];

fn alice() -> String {
    format!(
        "principal \"alice\" account=1001 login=[{}] approval=[{}] owned=[7] sets=[[],[7]] top=1000,30,300 contexts=4 idle=300",
        key_id(&ALICE_KEY),
        key_id(&ALICE_APPROVAL_KEY)
    )
}

fn bob() -> String {
    format!(
        "principal \"bob\" account=1002 login=[{}] approval=[22] owned=[] sets=[[]] top=500,10,100 contexts=2 idle=300",
        key_id(&BOB_KEY)
    )
}

const AGENT_KEY: [u8; 32] = [3; 32];

/// The image's agent principal: sponsored by alice, so it has no top of its own.
fn agent() -> String {
    format!(
        "principal \"agent\" account=1003 login=[{}] approval=[23] owned=[] sets=[[]] top=0,0,0 contexts=2 idle=300 sponsor=\"alice\"",
        key_id(&AGENT_KEY)
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
        kind: Kind,
        labels: Vec<u64>,
        slot: u16,
        scope: usize,
    },
    Console(Option<u32>),
    Release(usize),
    /// A connection's server told, at the loop's turn: `last` on the attempt past which the
    /// record goes anyway.
    Clean {
        handle: usize,
        last: bool,
    },
    Launch {
        kind: Kind,
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
    /// `lease_ended` sent on a session's notices endpoint.
    Notify {
        notices: usize,
        lease: u64,
        name: String,
    },
    /// `approval_waiting` sent on a session's notices endpoint.
    NotifyApproval(usize),
    /// An approval channel's two consoles, by the first's index.
    ApprovalConsoles(Option<u32>),
    ChannelOpened(u64),
    /// A screen written on an approval channel.
    Screen {
        channel: u64,
        text: String,
    },
    ChannelClosed(u64),
    /// `init`'s new connection at a restarted server.
    Reconnected {
        server: String,
    },
    /// `server_restarted` sent on a session's notices endpoint.
    Restarted {
        notices: usize,
        server: String,
        slot: u16,
    },
    /// A slot's connection made again; the handle made is the new connection's.
    Rebind {
        kind: Kind,
        labels: Vec<u64>,
        slot: u16,
        scope: usize,
    },
    /// The crossing program run in `budget` for a domain with `labels`, its connection at the
    /// vault's root, reading the item at `path` or writing `write` as it; the handles made are
    /// its connection's key, then its process.
    Cross {
        budget: usize,
        labels: Vec<u64>,
        path: String,
        write: Option<Vec<u8>>,
    },
    /// The steward's own look at, read of, or write to a file in a principal's home.
    HomeStat {
        principal: String,
        path: String,
    },
    HomeRead {
        principal: String,
        path: String,
    },
    HomeWrite {
        principal: String,
        path: String,
        bytes: Vec<u8>,
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
    /// Each budget made, with the deadline it was asked for.
    deadlines: Vec<(usize, u64)>,
    /// Budgets the kernel's deadline took: a `destroy` of one is `BadHandle`, its handle revoked.
    gone: Vec<usize>,
    /// Every reader thread is taken: the next channel cannot be served.
    channels_full: bool,
    /// What every cleanup step's server answers: `Ok` unless a test says (`Timeout`: the server
    /// did not take the call).
    clean: Option<Error>,
    /// Microseconds the clock has been moved on by, beside the words drawn.
    clock: u64,
    /// The files in the principals' homes, by principal and path: what the steward itself reads
    /// and writes.
    homes: BTreeMap<(String, String), Vec<u8>>,
    /// The vault's items, by path: what a crossing's program reads, and where it writes.
    vault: BTreeMap<String, Vec<u8>>,
    /// The crossing program dies, times out or answers wrongly: its call fails with this.
    cross_fails: Option<Error>,
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
        let made = self.make()?;
        self.deadlines.push((made, spec.deadline));
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
        if self.gone.contains(&budget) { Err(Error::BadHandle) } else { Ok(()) }
    }

    fn budget_id(&self, budget: usize) -> u64 { budget as u64 }

    fn mint(&mut self, badge: u64, stamp: usize) -> Result<usize, Error> {
        let made = self.make()?;
        self.calls.push(Call::Mint { badge, stamp });
        Ok(made)
    }

    /// The binding table's shape: slot 2 (the vault) binds to nothing unlabelled, slot 3 (the
    /// network) to nothing labelled; a lease binds its project root and the system volume alone.
    fn connect(&mut self, owner: &Object, slot: u16, scope: usize) -> Result<Option<usize>, Error> {
        let labels = owner.domain.labels().as_slice().to_vec();
        let binds = redoubt_steward_server::own::binds(Child::of(owner.kind, &labels), slot);
        self.calls.push(Call::Connect { kind: owner.kind, labels, slot, scope });
        if binds { self.make().map(Some) } else { Ok(None) }
    }

    fn console(&mut self, handle: Option<Handle>, relay: Option<Handle>) {
        assert_eq!(handle.is_some(), relay.is_some(), "a login brings both consoles or none");
        self.calls.push(Call::Console(handle.map(|h| h.index())));
    }

    fn release(&mut self, handle: usize) { self.calls.push(Call::Release(handle)); }

    fn clean(&mut self, handle: usize, last: bool) -> Result<(), Error> {
        self.calls.push(Call::Clean { handle, last });
        self.clean.map_or(Ok(()), Err)
    }

    fn launch(
        &mut self,
        owner: &Object,
        budget: usize,
        connections: &[Option<usize>],
        context: Option<&str>,
    ) -> Result<(u64, Option<usize>), Error> {
        // A session's VM is given a notices endpoint; the handle made is its send right. Made
        // before the process, so the last thing a login makes is its pid.
        let notices = if owner.kind == Kind::Session { Some(self.make()?) } else { None };
        let pid = self.make()? as u64;
        let context = context.map(String::from);
        self.calls.push(Call::Launch {
            kind: owner.kind,
            budget,
            connections: connections.to_vec(),
            context,
        });
        Ok((pid, notices))
    }

    fn notify(&mut self, notices: usize, lease: u64, name: &str) {
        self.calls.push(Call::Notify { notices, lease, name: name.into() });
    }

    fn notify_approval(&mut self, notices: usize) { self.calls.push(Call::NotifyApproval(notices)); }

    fn approval_consoles(&mut self, console: Option<Handle>, reader: Option<Handle>) {
        assert_eq!(console.is_some(), reader.is_some(), "an open brings both consoles or none");
        self.calls.push(Call::ApprovalConsoles(console.map(|h| h.index())));
    }

    fn channel_opened(&mut self, channel: u64) -> Result<(), Error> {
        self.calls.push(Call::ChannelOpened(channel));
        if self.channels_full { Err(Error::Refused) } else { Ok(()) }
    }

    fn screen(&mut self, channel: u64, text: &str) {
        self.calls.push(Call::Screen { channel, text: text.into() });
    }

    fn channel_closed(&mut self, channel: u64) { self.calls.push(Call::ChannelClosed(channel)); }

    fn reconnected(&mut self, server: &str, _connection: Handle) {
        self.calls.push(Call::Reconnected { server: server.into() });
    }

    /// The image's binding table by handle name: `/boot` at `bootfsd`, the home at `walfsd:data`,
    /// the vault at `littlefsd:alice-secrets`, the network at `ipd`; the console (slot 4) is no
    /// shared server, and the system volume (slot 5) a named handle nobody re-binds.
    fn slots_at(&self, owner: &Object, server: &str) -> Vec<u16> {
        let labels = owner.domain.labels().as_slice().to_vec();
        let slot = match server {
            "bootfsd" => 0,
            "walfsd:data" => 1,
            "littlefsd:alice-secrets" => 2,
            "ipd" => 3,
            _ => return Vec::new(),
        };
        if binds(Child::of(owner.kind, &labels), slot) { vec![slot] } else { Vec::new() }
    }

    fn restarted(&mut self, notices: usize, server: &str, slot: u16) {
        self.calls.push(Call::Restarted { notices, server: server.into(), slot });
    }

    fn rebind(
        &mut self,
        owner: &Object,
        slot: u16,
        scope: usize,
    ) -> Result<Option<(usize, Handle, String)>, Error> {
        let labels = owner.domain.labels().as_slice().to_vec();
        let child = Child::of(owner.kind, &labels);
        self.calls.push(Call::Rebind { kind: owner.kind, labels, slot, scope });
        let at = match slot {
            0 => "/boot",
            1 => "/home",
            2 => "/vault",
            3 => "/net",
            _ => return Ok(None),
        };
        if !binds(child, slot) {
            return Ok(None);
        }
        let made = self.make()?;
        Ok(Some((made, Handle::new(made as u32).unwrap(), at.into())))
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

    /// The crossing's program: the vault's item at `path` read, or `bytes` written as it; the
    /// connection's key is made first, the process after.
    fn cross(
        &mut self,
        owner: &Object,
        budget: usize,
        path: &str,
        bytes: Option<&[u8]>,
    ) -> Result<(u64, Option<usize>, Vec<u8>), Error> {
        self.calls.push(Call::Cross {
            budget,
            labels: owner.domain.labels().as_slice().to_vec(),
            path: path.into(),
            write: bytes.map(<[u8]>::to_vec),
        });
        if let Some(e) = self.cross_fails {
            return Err(e);
        }
        let key = self.make()?;
        let pid = self.make()? as u64;
        let read = match bytes {
            Some(bytes) => {
                self.vault.insert(path.into(), bytes.to_vec());
                Vec::new()
            }
            None => self.vault.get(path).cloned().ok_or(Error::Refused)?,
        };
        Ok((pid, Some(key), read))
    }

    fn home_stat(&mut self, principal: &str, path: &str) -> Result<Option<u64>, Error> {
        self.calls.push(Call::HomeStat { principal: principal.into(), path: path.into() });
        Ok(self.homes.get(&(principal.into(), path.into())).map(|b| b.len() as u64))
    }

    fn home_read(&mut self, principal: &str, path: &str, max: usize) -> Result<Vec<u8>, Error> {
        self.calls.push(Call::HomeRead { principal: principal.into(), path: path.into() });
        let bytes = self.homes.get(&(principal.into(), path.into())).ok_or(Error::Refused)?;
        if bytes.len() > max { Err(Error::TooLarge) } else { Ok(bytes.clone()) }
    }

    fn home_write(&mut self, principal: &str, path: &str, bytes: &[u8]) -> Result<(), Error> {
        self.calls.push(Call::HomeWrite {
            principal: principal.into(),
            path: path.into(),
            bytes: bytes.to_vec(),
        });
        let key = (principal.to_string(), path.to_string());
        if self.homes.contains_key(&key) {
            return Err(Error::Busy);
        }
        self.homes.insert(key, bytes.to_vec());
        Ok(())
    }

    fn random(&mut self) -> Result<u64, Error> {
        self.words += 1;
        Ok(self.words * 0x1_0000)
    }

    fn now(&mut self) -> u64 { self.words + self.clock }
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
    let mut l = vec![alice(), bob(), agent(), "keyd [31]".into(), SERVERS.into(), SIZES.into()];
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
        StartError::Lines("line 7: carol is no principal".into())
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
    let bob = bob().replace(
        &format!("login=[{}]", key_id(&BOB_KEY)),
        &format!("login=[{}]", key_id(&ALICE_APPROVAL_KEY)),
    );
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
    call_outputs(s, k, badge, m).0
}

/// [`call`], with the outputs beside the reply: the audit records and notices the call raised.
fn call_outputs(
    s: &mut Steward<usize, usize>,
    k: &mut Recorder,
    badge: u64,
    m: Message<'_>,
) -> (Result<Reply<'static>, ErrorCode>, Vec<Output>) {
    // An inline message travels in its words alone: one that came with a lend is malformed.
    let inline = matches!(
        m,
        Message::ChannelClosed(_)
            | Message::ApprovalClosed(_)
            | Message::Pending(_)
            | Message::EndLease(_)
            | Message::EndSession(_)
            | Message::Detach(_)
            | Message::Watch(_)
    );
    let mut buf = if inline { Vec::new() } else { vec![0u8; 4096] };
    let words = m.encode(&mut buf).unwrap();
    let opcode = words[0] as u32;
    let caller = Caller { badge, account: 0, labels: Default::default() };
    // A login brings the channel's two console connections, an approval channel's open its
    // two: here handles 9 and 10; `init`'s `reconnected` brings the new connection, handle 9.
    let mut handles = ReceivedHandles::new();
    if matches!(m, Message::Login(_) | Message::ApprovalOpened(_) | Message::Reconnected(_)) {
        handles.push(Handle::new(CONSOLE)).unwrap();
    }
    if matches!(m, Message::Login(_) | Message::ApprovalOpened(_)) {
        handles.push(Handle::new(CONSOLE + 1)).unwrap();
    }
    let mut serving = Serving::new(s, k);
    let outcome = answer_with(&mut serving, &caller, &words, &handles, &mut buf);
    assert!(!serving.exited, "the core exited");
    let outputs = core::mem::take(&mut serving.outputs);
    // What the server raises once the call is answered (an opened channel's first screen), as
    // the program's loop does.
    for kind in core::mem::take(&mut serving.after) {
        serving.steward.event(serving.kernel, kind, 0).unwrap();
    }
    // A reply that carries a handle (`rebind`'s connection) decodes only with it counted.
    let reply = Reply::decode(opcode, &outcome.words, &buf, outcome.send.as_slice().len()).unwrap();
    // Owned copies of what the tests read: a login's session and labels.
    let reply = reply.map(|r| match r {
        Reply::Login(l) => Reply::Login(LoginReply {
            session: l.session,
            name: Box::leak(l.name.to_string().into_boxed_str()),
            labels: Box::leak(l.labels.to_vec().into_boxed_slice()),
        }),
        Reply::ChannelClosed(_) => {
            Reply::ChannelClosed(redoubt_rt::wire::proto::steward::ChannelClosedReply {})
        }
        Reply::EndSession(_) => Reply::EndSession(redoubt_rt::wire::proto::steward::EndSessionReply {}),
        Reply::Contexts(c) => Reply::Contexts(redoubt_rt::wire::proto::steward::ContextsReply {
            list: Box::leak(c.list.to_string().into_boxed_str()),
        }),
        Reply::Detach(_) => Reply::Detach(redoubt_rt::wire::proto::steward::DetachReply {}),
        Reply::EndContext(_) => Reply::EndContext(redoubt_rt::wire::proto::steward::EndContextReply {}),
        Reply::StartAgent(a) => Reply::StartAgent(redoubt_rt::wire::proto::steward::StartAgentReply {
            lease: a.lease,
            name: Box::leak(a.name.to_string().into_boxed_str()),
        }),
        Reply::EndLease(_) => Reply::EndLease(redoubt_rt::wire::proto::steward::EndLeaseReply {}),
        Reply::Blame(_) => Reply::Blame(redoubt_rt::wire::proto::steward::BlameReply {}),
        Reply::Submit(r) => Reply::Submit(SubmitReply { request: r.request }),
        Reply::ApprovalOpened(r) => Reply::ApprovalOpened(ApprovalOpenedReply { channel: r.channel }),
        Reply::ApprovalClosed(_) => Reply::ApprovalClosed(ApprovalClosedReply {}),
        Reply::Pending(_) => Reply::Pending(PendingReply {}),
        Reply::Approve(_) => Reply::Approve(ApproveReply {}),
        Reply::Deny(_) => Reply::Deny(DenyReply {}),
        Reply::Reconnected(_) => Reply::Reconnected(ReconnectedReply {}),
        Reply::Rebind(r) => Reply::Rebind(RebindReply { at: Box::leak(r.at.to_string().into_boxed_str()) }),
        _ => panic!("a reply these tests do not read"),
    });
    (reply, outputs)
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
    let connects: Vec<Call> =
        (0..6).map(|slot| Call::Connect { kind: Kind::Session, labels: vec![], slot, scope }).collect();
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
        Call::Launch { kind: Kind::Session, budget, connections, context } => {
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
                kept: 18
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
    // `blame` is `init`'s alone: on any other badge it is malformed, so nobody can have another
    // principal's sessions ended by forging three crashes (servers/init.md, "Only init can blame").
    for badge in [session, SSHD, APPROVAL, 0, 4, MINTED - 1] {
        let m = Message::Blame(Blame { account: 1002, labels: &[], server: "walfsd:data" });
        assert_eq!(call(&mut s, &mut k, badge, m).unwrap_err(), ErrorCode::Malformed, "blame on {badge:#x}");
    }
    // The powerbox's: `submit` from a minted badge alone, the channel's open and close from
    // `sshd`'s alone, and `pending`, `approve` and `deny` from the approval channel's alone, which
    // only the steward's own readers hold (servers/steward.md, "The powerbox and approvals").
    for badge in [SSHD, APPROVAL, INIT, 0, 4, MINTED - 1] {
        let e = call(&mut s, &mut k, badge, submit(0, &[], 0, "", "x")).unwrap_err();
        assert_eq!(e, ErrorCode::Malformed, "submit on {badge:#x}");
    }
    for badge in [session, APPROVAL, INIT, 0, 4, MINTED - 1] {
        let m = Message::ApprovalOpened(ApprovalOpened { key: &ALICE_APPROVAL_KEY });
        assert_eq!(
            call(&mut s, &mut k, badge, m).unwrap_err(),
            ErrorCode::Malformed,
            "approval_opened on {badge:#x}"
        );
        let m = Message::ApprovalClosed(ApprovalClosed { channel: 7 });
        assert_eq!(
            call(&mut s, &mut k, badge, m).unwrap_err(),
            ErrorCode::Malformed,
            "approval_closed on {badge:#x}"
        );
    }
    for badge in [session, SSHD, INIT, 0, 4, MINTED - 1] {
        let m = Message::Pending(Pending { channel: 7 });
        assert_eq!(
            call(&mut s, &mut k, badge, m).unwrap_err(),
            ErrorCode::Malformed,
            "pending on {badge:#x}"
        );
        let m = Message::Approve(Approve { channel: 7, request: 8, hash: &[0; 32] });
        assert_eq!(
            call(&mut s, &mut k, badge, m).unwrap_err(),
            ErrorCode::Malformed,
            "approve on {badge:#x}"
        );
        let m = Message::Deny(Deny { channel: 7, request: 8 });
        assert_eq!(call(&mut s, &mut k, badge, m).unwrap_err(), ErrorCode::Malformed, "deny on {badge:#x}");
    }
    assert!(k.creates().is_empty());
    assert!(!k.calls.iter().any(|c| matches!(c, Call::ApprovalConsoles(_))), "no console taken off class");
    // On its own class, an operation from a minted badge the core routes nowhere is the core's
    // unknown; the steward's own `lease_ended` is a call of nobody's, on any badge.
    let e = call(&mut s, &mut k, session, Message::StartAgent(StartAgent { lease: 1 })).unwrap_err();
    assert_eq!(e, ErrorCode::Unknown);
    for badge in [session, SSHD, APPROVAL, INIT, 0] {
        let m = Message::LeaseEnded(LeaseEnded { lease: 1, name: "alice/agent-1" });
        assert_eq!(
            call(&mut s, &mut k, badge, m).unwrap_err(),
            ErrorCode::Malformed,
            "lease_ended on {badge:#x}"
        );
    }
    // The restart path (servers/steward.md, "Failure and restart"): `reconnected` is `init`'s
    // alone, so nobody else can swap the connection the steward mints every home through;
    // `rebind` is a session's; the steward's own `server_restarted` is a call of nobody's.
    for badge in [session, SSHD, APPROVAL, 0, 4, MINTED - 1] {
        let m = Message::Reconnected(Reconnected { server: "walfsd:data" });
        assert_eq!(
            call(&mut s, &mut k, badge, m).unwrap_err(),
            ErrorCode::Malformed,
            "reconnected on {badge:#x}"
        );
    }
    for badge in [SSHD, APPROVAL, INIT, 0] {
        let m = Message::Rebind(Rebind { slot: 1 });
        assert_eq!(call(&mut s, &mut k, badge, m).unwrap_err(), ErrorCode::Malformed, "rebind on {badge:#x}");
    }
    for badge in [session, SSHD, APPROVAL, INIT, 0] {
        let m = Message::ServerRestarted(ServerRestarted { server: "walfsd:data", slot: 1 });
        assert_eq!(
            call(&mut s, &mut k, badge, m).unwrap_err(),
            ErrorCode::Malformed,
            "server_restarted on {badge:#x}"
        );
    }
    assert!(k.calls.iter().all(|c| !matches!(c, Call::Reconnected { .. } | Call::Rebind { .. })));
}

/// A shared server's restart (servers/steward.md, "Failure and restart"): `init`'s `reconnected`
/// hands the kernel the new connection, and every live session with a slot at that server is
/// sent `server_restarted` for it: at `walfsd:data`, slot 1 (the home) to alice's two sessions
/// and bob's, nothing to the lease, which has no notices endpoint; at the vault's server, to the
/// vault session alone. A session's `rebind(1)` re-makes that slot's connection in the session's
/// scope and answers the handle and the path; the dead one's record is released (the kernel has
/// nothing to disconnect) and the new one is held in its place, so the session's end gives it
/// back. The console's slot, the named handle's, a slot bound to nothing for the caller and a lease's badge
/// are `unknown`, as is a badge the core routes nowhere.
#[test]
fn a_servers_restart_tells_its_sessions_and_a_session_rebinds_its_slot() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let Ok(Reply::Login(a)) = call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)) else { panic!() };
    let u = badge_of(&s, a.session);
    // The login's connection at slot 1, and the scope it was minted in.
    let login_scope = k
        .calls
        .iter()
        .find_map(|c| match c {
            Call::Connect { slot: 1, scope, .. } => Some(*scope),
            _ => None,
        })
        .expect("the login minted the home's connection in the session's scope");
    k.calls.clear();
    let Ok(Reply::Login(b)) = call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)) else { panic!() };
    let b = badge_of(&s, b.session);
    // The last thing a login makes is its pid.
    let bob_pid = k.made as u64;
    let Ok(Reply::Login(v)) = call(&mut s, &mut k, SSHD, login("alice", "alice-secrets", &ALICE_KEY)) else {
        panic!()
    };
    let v = badge_of(&s, v.session);
    let Ok(Reply::StartAgent(_)) =
        call(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 }))
    else {
        panic!()
    };
    let before = s.holding();
    k.calls.clear();
    let m = Message::Reconnected(Reconnected { server: "walfsd:data" });
    let Ok(Reply::Reconnected(_)) = call(&mut s, &mut k, INIT, m) else { panic!() };
    assert_eq!(k.calls[0], Call::Reconnected { server: "walfsd:data".into() });
    let told = |k: &Recorder, server: &str| -> Vec<(usize, u16)> {
        k.calls
            .iter()
            .filter_map(|c| match c {
                Call::Restarted { notices, server: at, slot } if at == server => Some((*notices, *slot)),
                _ => None,
            })
            .collect()
    };
    let homes = told(&k, "walfsd:data");
    assert_eq!(homes.len(), 3, "alice's two sessions and bob's, not the lease: {:?}", k.calls);
    assert!(homes.iter().all(|(_, slot)| *slot == 1), "{homes:?}");
    let mut endpoints: Vec<usize> = homes.iter().map(|(n, _)| *n).collect();
    endpoints.dedup();
    assert_eq!(endpoints.len(), 3, "each on its own notices endpoint");
    assert_eq!(k.calls.len(), 4, "nothing else: {:?}", k.calls);
    k.calls.clear();
    let m = Message::Reconnected(Reconnected { server: "littlefsd:alice-secrets" });
    let Ok(Reply::Reconnected(_)) = call(&mut s, &mut k, INIT, m) else { panic!() };
    assert_eq!(told(&k, "littlefsd:alice-secrets").len(), 1, "the vault session alone: {:?}", k.calls);
    k.calls.clear();
    let Ok(Reply::Rebind(r)) = call(&mut s, &mut k, u, Message::Rebind(Rebind { slot: 1 })) else { panic!() };
    assert_eq!(r.at, "/home");
    // By the login batch's route: the same object, the same slot, the same scope, so the same
    // stamp, root and quota as the login's connection (the binding table is one function of
    // them); the re-bind grants nothing the login did not.
    assert_eq!(
        k.calls[0],
        Call::Rebind { kind: Kind::Session, labels: vec![], slot: 1, scope: login_scope },
        "{:?}",
        k.calls
    );
    assert!(matches!(k.calls[1], Call::Release(_)), "the dead connection's record goes: {:?}", k.calls);
    assert_eq!(k.calls.len(), 2);
    assert_eq!(s.holding(), before, "the new connection takes the dead one's token");
    for slot in [2, 4, 5, 6] {
        let e = call(&mut s, &mut k, u, Message::Rebind(Rebind { slot })).unwrap_err();
        assert_eq!(
            e,
            ErrorCode::Unknown,
            "slot {slot}: the vault binds nothing unlabelled, the rest no path"
        );
    }
    let e = call(&mut s, &mut k, MINTED | 5, Message::Rebind(Rebind { slot: 1 })).unwrap_err();
    assert_eq!(e, ErrorCode::Unknown);
    // A lease is not re-bound (Q3 of the ruling): its badge's `rebind` of its project root is
    // answered as a foreign slot's, and nothing is made or released.
    let lease_badge = redoubt_steward::inspect::index(&s.store)
        .routes
        .iter()
        .find(|(_, r)| r.kind == Kind::Lease)
        .map(|(b, _)| *b)
        .expect("the lease is routed");
    let held = s.holding();
    k.calls.clear();
    let e = call(&mut s, &mut k, lease_badge, Message::Rebind(Rebind { slot: 1 })).unwrap_err();
    assert_eq!(e, ErrorCode::Unknown, "a lease's rebind");
    assert!(k.calls.is_empty() && s.holding() == held, "nothing made or released: {:?}", k.calls);
    let Ok(Reply::Rebind(_)) = call(&mut s, &mut k, v, Message::Rebind(Rebind { slot: 2 })) else { panic!() };
    let Ok(Reply::Rebind(_)) = call(&mut s, &mut k, b, Message::Rebind(Rebind { slot: 1 })) else { panic!() };
    let bobs_new = k.made;
    k.calls.clear();
    let object = s.exited(bob_pid).expect("bob's VM runs");
    s.event(&mut k, EventKind::Exited { object }, 0).unwrap();
    assert!(
        k.calls.contains(&Call::Release(bobs_new)),
        "the re-bound connection goes with the session: {:?}",
        k.calls
    );
    assert!(s.failed.is_empty());
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
            Call::Attach { relay, note: "[context default: reattached]\r\n".into(), kept: 18 },
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

/// R41's order at a session's end (servers/steward.md, "R41"): the budget, with the scope its
/// connections were minted in, is destroyed first, the cutoff the kernel makes in every table
/// at once; then each connection is given back to its server, the cleanup. Every connection the
/// session's batch made goes back, the steward's own badge and the console included, and the
/// steward holds nothing of the session afterwards.
#[test]
fn a_session_s_end_destroys_its_budget_before_it_releases_its_connections() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let before = s.holding();
    let Ok(Reply::Login(_)) = call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)) else { panic!() };
    let scope = k.calls.iter().find_map(|c| match c {
        Call::Connect { scope, .. } => Some(*scope),
        _ => None,
    });
    assert!(scope.is_some(), "every shared connection is minted in the session's scope");
    let pid = k.made as u64;
    let object = s.exited(pid).unwrap();
    k.calls.clear();
    s.event(&mut k, redoubt_steward::event::EventKind::Exited { object }, 0).unwrap();
    let destroyed = k.calls.iter().position(|c| matches!(c, Call::Destroy(_))).expect("the budget goes");
    let releases: Vec<usize> =
        k.calls.iter().enumerate().filter(|(_, c)| matches!(c, Call::Release(_))).map(|(i, _)| i).collect();
    // Every connection the batch held: the badge, the shared slots, the relay's control
    // connection and the console.
    assert!(releases.len() >= 6, "{:?}", k.calls);
    assert!(releases.iter().all(|&i| i > destroyed), "destroy, then disconnect: {:?}", k.calls);
    assert_eq!(s.holding(), before, "nothing of the session is held");
}

/// The handles `k` recorded as given back (`release`) or told (`clean`), in order.
fn released(k: &Recorder) -> Vec<usize> {
    k.calls.iter().filter_map(|c| if let Call::Release(h) = c { Some(*h) } else { None }).collect()
}

fn cleaned(k: &Recorder) -> Vec<(usize, bool)> {
    k.calls
        .iter()
        .filter_map(|c| if let Call::Clean { handle, last } = c { Some((*handle, *last)) } else { None })
        .collect()
}

/// A session logged in and ended by its own `end_session`, with the kernel's record cleared
/// before the end: what the end batch left in the cleanup queue.
fn ended_session(s: &mut Steward<usize, usize>, k: &mut Recorder, principal: &str, key: &[u8]) {
    let Ok(Reply::Login(r)) = call(s, k, SSHD, login(principal, "", key)) else { panic!() };
    let badge = badge_of(s, r.session);
    k.calls.clear();
    assert!(matches!(call(s, k, badge, Message::EndSession(EndSession {})), Ok(Reply::EndSession(_))));
}

/// The end batch is answered once its scope is destroyed, the cutoff; the servers are told at
/// the loop's turn (servers/steward.md, "Failure and restart"): no `clean` runs inside the event,
/// every connection the end released waits in the queue, and one turn tells them all, once each,
/// in the order they were released.
#[test]
fn a_session_s_end_is_answered_before_any_cleanup_call() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    let released = released(&k);
    assert!(released.len() >= 6, "{:?}", k.calls);
    assert!(cleaned(&k).is_empty(), "no server is told inside the event: {:?}", k.calls);
    assert_eq!(s.pending(), released.len());
    k.calls.clear();
    assert_eq!(s.cleanup(&mut k), None, "nothing is left to retry");
    assert_eq!(cleaned(&k), released.iter().map(|&h| (h, false)).collect::<Vec<_>>());
    assert_eq!(s.pending(), 0);
    k.calls.clear();
    assert_eq!(s.cleanup(&mut k), None);
    assert!(k.calls.is_empty(), "an empty queue makes no call");
}

/// What the end batch defers is the servers' side alone: the core's events of the end, a
/// session's pending requests withdrawn with it (servers/steward.md, "The powerbox and
/// approvals"), all run before the end is answered. Alice's session submits a request and ends;
/// an approval channel opened on the reply, before any cleanup turn, lists nothing of hers.
#[test]
fn an_ended_session_s_pending_request_is_withdrawn_before_the_end_s_reply() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let Ok(Reply::Submit(r)) = call(&mut s, &mut k, u, submit(0, &[], 0, "", "mine")) else { panic!() };
    k.calls.clear();
    assert!(matches!(call(&mut s, &mut k, u, Message::EndSession(EndSession {})), Ok(Reply::EndSession(_))));
    assert!(cleaned(&k).is_empty() && s.pending() > 0, "the servers are still to be told: {:?}", k.calls);
    let c = open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap();
    let screen = screens(&k, c).pop().expect("the channel's first screen");
    assert!(
        !screen.contains(&format!("request {:016x} ", r.request)),
        "withdrawn with her session: {screen}"
    );
    assert!(!screen.contains("\"mine\""), "{screen}");
    assert!(screen.starts_with("no approval is waiting for alice"), "{screen}");
}

/// One round of attempts at `steps` due steps whose servers all time out: a turn tells one step
/// and leaves the rest due now, so the round is as many turns as steps; the last turn's due.
fn round(s: &mut Steward<usize, usize>, k: &mut Recorder, steps: usize) -> Option<u64> {
    let now = k.now();
    let mut due = None;
    for turn in 0..steps {
        let before = cleaned(k).len();
        due = s.cleanup(k);
        assert_eq!(cleaned(k).len(), before + 1, "turn {turn}: one timed-out step per turn");
        if turn + 1 < steps {
            assert!(due.is_some_and(|t| t <= now), "turn {turn}: the steps behind it stay due: {due:?}");
        }
    }
    due
}

/// A step whose server does not take the call within the steward's send bound (`Timeout`) is
/// kept and retried no sooner than `RETRY_US` later, up to `TRIES` attempts, the last told it is
/// the last; then the record goes whatever the server did. A turn before the retry is due makes
/// no call.
#[test]
fn a_cleanup_step_that_times_out_is_retried_then_dropped() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    let pending = s.pending();
    assert!(pending > 0);
    k.clean = Some(Error::Timeout);
    k.calls.clear();
    let now = k.now();
    let due = round(&mut s, &mut k, pending);
    assert_eq!(due, Some(now + RETRY_US), "the retry is due RETRY_US after the attempt");
    assert_eq!(cleaned(&k).len(), pending);
    assert!(cleaned(&k).iter().all(|&(_, last)| !last));
    assert_eq!(s.pending(), pending, "a timed-out step is kept");
    k.calls.clear();
    k.clock += RETRY_US - 1;
    assert_eq!(s.cleanup(&mut k), due);
    assert!(k.calls.is_empty(), "not due yet: no call");
    for attempt in 2..=TRIES {
        k.clock += RETRY_US;
        k.calls.clear();
        let next = round(&mut s, &mut k, pending);
        let last = attempt == TRIES;
        assert_eq!(cleaned(&k).len(), pending, "attempt {attempt}");
        assert!(cleaned(&k).iter().all(|&(_, l)| l == last), "attempt {attempt}: {:?}", k.calls);
        if last {
            assert_eq!((next, s.pending()), (None, 0), "dropped after TRIES attempts");
        } else {
            assert!(next.is_some() && s.pending() == pending, "attempt {attempt} is kept");
        }
    }
}

/// A turn stops at its first step the server did not take in time: the steps behind it stay
/// due now, untold, so the loop receives before it waits on a server again, and a turn costs it
/// one send bound at most; the next turn, its servers answering, tells every other step.
#[test]
fn a_turn_stops_at_its_first_timed_out_step() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    let pending = s.pending();
    assert!(pending >= 2);
    let first = released(&k)[0];
    k.clean = Some(Error::Timeout);
    k.calls.clear();
    let now = k.now();
    assert_eq!(s.cleanup(&mut k), Some(now), "the steps behind the timed-out one are due now");
    assert_eq!(cleaned(&k), vec![(first, false)], "one wait per turn: {:?}", k.calls);
    assert_eq!(s.pending(), pending, "the timed-out step is kept, the rest untouched");
    k.clean = None;
    k.calls.clear();
    assert_eq!(s.cleanup(&mut k), Some(now + RETRY_US), "only the retry is left");
    assert_eq!(cleaned(&k).len(), pending - 1, "the next turn tells every other step: {:?}", k.calls);
    assert_eq!(s.pending(), 1);
}

/// Any outcome but `Timeout` ends a step at once: an `ended` to a channel `sshd` already closed
/// fails `Dead` and is never retried, and a `disconnect` the server refuses is done too.
#[test]
fn a_cleanup_step_the_server_refuses_or_cannot_take_is_done_at_once() {
    for error in [Error::Dead, Error::Refused] {
        let mut k = Recorder::default();
        let mut s = started(&mut k);
        ended_session(&mut s, &mut k, "bob", &BOB_KEY);
        let pending = s.pending();
        k.clean = Some(error);
        k.calls.clear();
        assert_eq!(s.cleanup(&mut k), None);
        assert_eq!(cleaned(&k).len(), pending);
        assert_eq!(s.pending(), 0, "{error:?} ends the step");
    }
}

/// The queue's bound (servers/steward.md, "Failure and restart"): a connection enters once, when
/// its owner's end releases it, so the queue never holds more than the connections the ended
/// objects held, at most `SLOTS + 4` per session (its badge, a connection per slot, its relay's
/// control connection, the attached channel console and its notices send right); a turn empties
/// it, and an object's second end queues nothing.
#[test]
fn the_cleanup_queue_holds_each_released_connection_once() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let mut all = Vec::new();
    for (principal, key) in [("bob", &BOB_KEY), ("alice", &ALICE_KEY)] {
        ended_session(&mut s, &mut k, principal, key);
        let released = released(&k);
        assert!(released.len() <= SLOTS as usize + 4, "{principal}: {released:?}");
        all.extend(released);
    }
    let mut unique = all.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), all.len(), "each connection once: {all:?}");
    assert_eq!(s.pending(), all.len());
    k.calls.clear();
    s.cleanup(&mut k);
    assert_eq!(cleaned(&k).len(), all.len());
    assert_eq!(s.pending(), 0);
}

/// The badge a session's own calls come on: its route in the core's index.
fn badge_of(s: &Steward<usize, usize>, session: u64) -> u64 {
    let index = redoubt_steward::inspect::index(&s.store);
    index.routes.iter().find(|(_, r)| r.id == session).map(|(b, _)| *b).expect("a running session")
}

fn bob_in(context: &str) -> Message<'_> {
    Message::Login(Login { principal: "bob", label: "", context, from: "198.51.100.2:51234", key: &BOB_KEY })
}

/// The cap, through the server: bob's cap is 2; a third context is refused `cap` and makes
/// nothing, and the two he has stay.
#[test]
fn a_login_past_the_cap_is_refused_and_makes_nothing() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    call(&mut s, &mut k, SSHD, bob_in("a")).unwrap();
    call(&mut s, &mut k, SSHD, bob_in("b")).unwrap();
    k.calls.clear();
    assert_eq!(call(&mut s, &mut k, SSHD, bob_in("c")).unwrap_err(), ErrorCode::Cap);
    assert!(
        k.creates().is_empty() && !k.calls.iter().any(|c| matches!(c, Call::Destroy(_))),
        "{:?}",
        k.calls
    );
    // Reattaching one of the two is no new context.
    assert!(call(&mut s, &mut k, SSHD, bob_in("a")).is_ok());
}

/// A session's own calls on its badge (servers/steward.md, "Contexts"): its listing, one line a
/// context of its domain only, `name<TAB>state<TAB>seconds`; its own detach, which lets its
/// channel go and gives the console back; and its end of a context by name, which is `unknown`
/// for any name outside its domain, as for one nobody holds.
#[test]
fn a_session_lists_detaches_and_ends_its_own_domain_s_contexts() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let Ok(Reply::Login(a)) = call(&mut s, &mut k, SSHD, bob_in("a")) else { panic!() };
    let Ok(Reply::Login(b)) = call(&mut s, &mut k, SSHD, bob_in("b")) else { panic!() };
    let vault = Message::Login(Login {
        principal: "alice",
        label: "alice-secrets",
        context: "v",
        from: "",
        key: &ALICE_KEY,
    });
    call(&mut s, &mut k, SSHD, vault).unwrap();
    let badge = badge_of(&s, b.session);
    let r = call(&mut s, &mut k, badge, Message::Contexts(Contexts {}));
    let Ok(Reply::Contexts(c)) = r else { panic!("{r:?}") };
    assert_eq!(c.list, "a\tattached\t0\nb\tattached\t0\n");
    for name in ["v", "nobody"] {
        let e = call(&mut s, &mut k, badge, Message::EndContext(EndContext { name })).unwrap_err();
        assert_eq!(e, ErrorCode::Unknown, "{name}");
    }
    k.calls.clear();
    assert!(call(&mut s, &mut k, badge, Message::EndContext(EndContext { name: "a" })).is_ok());
    assert!(k.calls.iter().any(|c| matches!(c, Call::Destroy(_))), "{:?}", k.calls);
    let routed = redoubt_steward::inspect::index(&s.store).routes.values().any(|r| r.id == a.session);
    assert!(!routed, "a ended");
    k.calls.clear();
    assert!(call(&mut s, &mut k, badge, Message::Detach(Detach {})).is_ok());
    assert!(matches!(k.calls.as_slice(), [Call::Detach { .. }, Call::Release(_)]), "{:?}", k.calls);
    assert_eq!(call(&mut s, &mut k, badge, Message::Detach(Detach {})).unwrap_err(), ErrorCode::Unknown);
    // On `sshd`'s badge they are malformed.
    assert_eq!(call(&mut s, &mut k, SSHD, Message::Contexts(Contexts {})).unwrap_err(), ErrorCode::Malformed);
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
    // Agent, sponsored by alice, is carved nothing of its own.
    let names: Vec<&str> = s.carved.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["alice", "bob"]);
    let fixed = redoubt_steward::inspect::fixed(&s.store);
    assert_eq!(fixed.principals.iter().map(|p| p.sponsor).collect::<Vec<_>>(), [None, None, Some(0)]);
    assert_eq!(s.own.labels, [("alice-secrets".to_string(), 7)]);
    assert_eq!(s.own.console.as_deref(), Some("alice"));
    assert_eq!(s.own.net("alice").unwrap().rules, ["0.0.0.0/0:80,443"]);
    assert!(s.label("alice-secrets").is_some());
}

// ---- leases and agents (servers/steward.md, "Leases") ----

/// Alice's unlabelled session, logged in: its minted badge.
fn alice_session(s: &mut Steward<usize, usize>, k: &mut Recorder) -> u64 {
    let Ok(Reply::Login(r)) = call(s, k, SSHD, login("alice", "", &ALICE_KEY)) else { panic!() };
    let badge = badge_of(s, r.session);
    k.calls.clear();
    badge
}

/// The pid of the one process `s` runs for `kind`.
fn pid_of(s: &Steward<usize, usize>, kind: Kind) -> u64 {
    (1..200).find(|p| s.exited(*p).is_some_and(|o| o.kind == kind)).expect("a process of that kind")
}

/// `start_agent` from an unlabelled session's badge is the core's lease batch: the lease's
/// budget from the sponsor's unlabelled sub-budget with the agent's size and the deadline asked
/// for, a scope, the steward's badge on the scope, a connection per slot (a lease binds `/boot`,
/// its project root and the system volume) and the launch of the agent's VM with no notices
/// endpoint; the reply names the lease. A lease over `MAX_LEASE` is refused, never shortened; a
/// vault session starts nothing.
#[test]
fn start_agent_runs_the_lease_batch_and_answers_the_lease() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let before = k.deadlines.len();
    let Ok(Reply::StartAgent(a)) =
        call(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 }))
    else {
        panic!()
    };
    assert!(a.lease >= MINTED);
    assert_eq!(a.name, "agent-1");
    let alice_unlabelled = 2;
    assert_eq!(
        k.calls[0],
        Call::Create { parent: alice_unlabelled, pages: 10, processes: 2, labels: vec![], account: 0 }
    );
    let (budget, deadline) = k.deadlines[before];
    assert!(deadline != FOREVER && deadline >= 60_000_000, "a deadline, the lease's: {deadline}");
    assert!(matches!(k.calls[1], Call::Create { parent, pages: 0, .. } if parent == budget), "the scope");
    let scope = match &k.calls[2] {
        Call::Mint { stamp, .. } => *stamp,
        other => panic!("{other:?}"),
    };
    let connects: Vec<Call> =
        (0..6).map(|slot| Call::Connect { kind: Kind::Lease, labels: vec![], slot, scope }).collect();
    assert_eq!(&k.calls[3..9], &connects[..]);
    match &k.calls[9] {
        Call::Launch { kind: Kind::Lease, budget: b, connections, .. } => {
            assert_eq!(*b, budget);
            let bound: Vec<bool> = connections.iter().map(Option::is_some).collect();
            assert_eq!(
                bound,
                [true, true, true, false, false, false, true],
                "the steward's, /boot, slot 1, slot 5"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(k.calls.len(), 10, "no notices endpoint for an agent, nothing else");
    let too_long = Message::StartAgent(StartAgent { lease: MAX_LEASE + 1 });
    assert_eq!(call(&mut s, &mut k, u, too_long).unwrap_err(), ErrorCode::BadLease);
    let Ok(Reply::Login(v)) = call(&mut s, &mut k, SSHD, login("alice", "alice-secrets", &ALICE_KEY)) else {
        panic!()
    };
    let v = badge_of(&s, v.session);
    let from_vault = Message::StartAgent(StartAgent { lease: 1_000_000 });
    assert_eq!(call(&mut s, &mut k, v, from_vault).unwrap_err(), ErrorCode::Labelled);
}

/// `end_lease` is the sponsor's alone: bob's session is told nothing (`unknown`), alice's vault
/// session `not_sponsor`; from her unlabelled session it destroys the lease's budget, gives its
/// connections back, and sends `lease_ended`, named `alice/agent-1`, on the notices endpoint of
/// each of her unlabelled sessions and no other. The loop answers it ahead of admission.
#[test]
fn end_lease_from_the_sponsor_destroys_the_lease_and_tells_her_sessions() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let Ok(Reply::Login(b)) = call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)) else { panic!() };
    let b = badge_of(&s, b.session);
    let Ok(Reply::Login(v)) = call(&mut s, &mut k, SSHD, login("alice", "alice-secrets", &ALICE_KEY)) else {
        panic!()
    };
    let v = badge_of(&s, v.session);
    let Ok(Reply::StartAgent(a)) =
        call(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 }))
    else {
        panic!()
    };
    let lease = a.lease;
    let end = || Message::EndLease(EndLease { lease });
    let words = end().encode(&mut []).unwrap();
    let caller = |badge| Caller { badge, account: 0, labels: Default::default() };
    assert!(redoubt_steward_server::protocol::ahead(&caller(u), &words));
    assert!(!redoubt_steward_server::protocol::ahead(&caller(SSHD), &words));
    assert_eq!(call(&mut s, &mut k, b, end()).unwrap_err(), ErrorCode::Unknown);
    assert_eq!(call(&mut s, &mut k, v, end()).unwrap_err(), ErrorCode::NotSponsor);
    let pid = pid_of(&s, Kind::Lease);
    k.calls.clear();
    let Ok(Reply::EndLease(_)) = call(&mut s, &mut k, u, end()) else { panic!() };
    // The lease's process is no longer a running one, but its exit notice, still to come, gets
    // its line under the notice's name: kept for it once, then gone.
    assert!(s.exited(pid).is_none());
    let (object, name) = s.ended(pid).expect("the ended process is kept for its exit notice");
    assert!(object.kind == Kind::Lease && object.id == lease && name == "alice/agent-1", "{object:?} {name}");
    assert!(s.ended(pid).is_none());
    let notified: Vec<&Call> = k.calls.iter().filter(|c| matches!(c, Call::Notify { .. })).collect();
    assert_eq!(notified.len(), 1, "alice's one unlabelled session, not bob's, not her vault's: {notified:?}");
    assert!(
        matches!(notified[0], Call::Notify { lease: l, name, .. } if *l == lease && name == "alice/agent-1")
    );
    assert!(k.calls.iter().any(|c| matches!(c, Call::Destroy(_))));
    assert!(k.calls.iter().any(|c| matches!(c, Call::Release(_))), "its connections are given back");
    assert!(s.failed.is_empty());
}

/// A lease the kernel's deadline took: its process's exit notice arrives with the budget gone,
/// and the core's `DestroyBudget` on it is the end asked for, not a failed step; the sponsor is
/// told all the same.
#[test]
fn a_leases_deadline_ends_it_without_a_failed_step() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let Ok(Reply::StartAgent(a)) =
        call(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 2_000_000 }))
    else {
        panic!()
    };
    // The lease's budget: the one made with a deadline (its scope follows, for ever).
    let leased = |k: &Recorder| k.deadlines.iter().rev().find(|d| d.1 != FOREVER).map(|d| d.0).unwrap();
    let budget = leased(&k);
    let pid = pid_of(&s, Kind::Lease);
    k.gone.push(budget);
    k.calls.clear();
    let object = s.exited(pid).unwrap();
    let outputs = s.event(&mut k, EventKind::Exited { object }, 0).unwrap();
    assert!(s.failed.is_empty(), "{:?}", s.failed);
    assert!(k.calls.contains(&Call::Destroy(budget)), "{:?}", k.calls);
    assert!(k.calls.iter().any(
        |c| matches!(c, Call::Notify { lease, name, .. } if *lease == a.lease && name == "alice/agent-1")
    ));
    assert!(
        !outputs.iter().any(|o| matches!(o, Output::Notice { .. })),
        "the notice went, it is not left over"
    );
    // A destroy refused on a budget whose process still runs is a failed step.
    let Ok(Reply::StartAgent(a)) =
        call(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 2_000_000 }))
    else {
        panic!()
    };
    k.gone.push(leased(&k));
    let Ok(Reply::EndLease(_)) = call(&mut s, &mut k, u, Message::EndLease(EndLease { lease: a.lease }))
    else {
        panic!()
    };
    assert_eq!(s.failed.len(), 1);
}

/// A login as the sponsored principal is a lease of its sponsor's: its session's budget is
/// carved from alice's unlabelled sub-budget with a deadline of `MAX_LEASE`, in its own domain
/// and with its own name; alice's session ends it by `end_lease` with the session's id, and is
/// told `agent/session-1`; the launch is a session's, with a notices endpoint.
#[test]
fn a_login_as_the_sponsored_principal_is_a_lease_of_its_sponsors() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let before = k.deadlines.len();
    let Ok(Reply::Login(r)) = call(&mut s, &mut k, SSHD, login("agent", "", &AGENT_KEY)) else { panic!() };
    assert_eq!(r.name, "session-1");
    let alice_unlabelled = 2;
    assert!(
        matches!(k.calls[1], Call::Create { parent, pages: 10, processes: 2, .. } if parent == alice_unlabelled)
    );
    let (_, deadline) = k.deadlines[before];
    assert!(deadline != FOREVER && deadline >= MAX_LEASE, "a day's deadline: {deadline}");
    assert!(k.calls.iter().any(|c| matches!(c, Call::Launch { kind: Kind::Session, .. })));
    k.calls.clear();
    let Ok(Reply::EndLease(_)) = call(&mut s, &mut k, u, Message::EndLease(EndLease { lease: r.session }))
    else {
        panic!()
    };
    assert!(k.calls.iter().any(
        |c| matches!(c, Call::Notify { lease, name, .. } if *lease == r.session && name == "agent/session-1")
    ));
    assert!(k.calls.iter().any(|c| matches!(c, Call::Destroy(_))));
}

// ---- crash blame (servers/steward.md, "Crash blame", R40) ----

fn blame<'a>(account: u64, labels: &'a [u8]) -> Message<'a> {
    Message::Blame(Blame { account, labels, server: "walfsd:data" })
}

fn records(outputs: &[Output]) -> Vec<Record> {
    outputs
        .iter()
        .filter_map(|o| match o {
            Output::Audit(a) => Some(a.record().clone()),
            _ => None,
        })
        .collect()
}

/// `init`'s `blame` is counted by (account, label set): two blames on bob's unlabelled domain
/// change nothing but the count, each answered and audited `Blamed`; the third within the window
/// is the lockout, audited `LockedOut` with the window's end, which destroys the budgets of
/// bob's session and his lease and nothing of alice's; his next login is refused `locked_out`
/// while alice's is admitted, and once the window has passed his login is admitted again. A
/// blame on nobody (account 0) or on a label set the manifest does not know is answered the
/// same and counts nowhere. Keyed by the label set: three blames on alice's unlabelled domain
/// destroy her unlabelled session alone, her vault session stands and her vault login is
/// admitted while her unlabelled one is refused.
#[test]
fn three_blames_in_the_window_lock_the_domain_out_until_the_window_passes() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let alice_session_budget = k.made + 1;
    let _alice = alice_session(&mut s, &mut k);
    let bob_session = k.made + 1;
    let Ok(Reply::Login(b)) = call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)) else { panic!() };
    let b = badge_of(&s, b.session);
    let bob_lease = k.made + 1;
    let Ok(Reply::StartAgent(_)) =
        call(&mut s, &mut k, b, Message::StartAgent(StartAgent { lease: 60_000_000 }))
    else {
        panic!()
    };
    k.calls.clear();
    for n in 1..BLAME_COUNT {
        let (reply, outputs) = call_outputs(&mut s, &mut k, INIT, blame(1002, &[]));
        assert!(matches!(reply, Ok(Reply::Blame(_))), "blame {n}: {reply:?}");
        assert_eq!(records(&outputs), vec![Record::Blamed], "blame {n}");
        assert!(k.calls.is_empty(), "blame {n} changes nothing visible: {:?}", k.calls);
    }
    let (reply, outputs) = call_outputs(&mut s, &mut k, INIT, blame(1002, &[]));
    assert!(matches!(reply, Ok(Reply::Blame(_))));
    let recorded = records(&outputs);
    let Some(Record::LockedOut { until }) = recorded.get(1).cloned() else { panic!("{recorded:?}") };
    assert!(until > k.words && until <= k.words + BLAME_WINDOW, "the window's end: {until} at {}", k.words);
    assert!(
        matches!(recorded[..], [Record::Blamed, Record::LockedOut { .. }, Record::LeaseEnded { .. }]),
        "{recorded:?}"
    );
    let destroyed: Vec<usize> =
        k.calls.iter().filter_map(|c| if let Call::Destroy(b) = c { Some(*b) } else { None }).collect();
    assert_eq!(destroyed, vec![bob_session, bob_lease], "bob's session and lease, nothing else");
    // The destroy took the relay with the budget: nobody is told, the console alone is given
    // back, so the blame's answer does not wait out a dead relay's bound (init's second).
    assert!(
        k.calls.iter().all(|c| !matches!(c, Call::Detach { .. })),
        "no detach call on a relay the destroy took: {:?}",
        k.calls
    );
    assert!(s.failed.is_empty());
    k.calls.clear();
    assert_eq!(call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)).unwrap_err(), ErrorCode::LockedOut);
    assert!(k.creates().is_empty(), "a refused login makes nothing");
    assert!(matches!(
        call(&mut s, &mut k, SSHD, login("alice", "alice-secrets", &ALICE_KEY)),
        Ok(Reply::Login(_))
    ));
    // Blame on nobody, and on a label set the manifest does not know: taken, counted nowhere.
    k.calls.clear();
    for (account, labels) in [(0, &[][..]), (1001, &99u64.to_le_bytes()[..])] {
        let (reply, outputs) = call_outputs(&mut s, &mut k, INIT, blame(account, labels));
        assert!(matches!(reply, Ok(Reply::Blame(_))));
        assert!(records(&outputs).is_empty() && k.calls.is_empty(), "{account}: {outputs:?} {:?}", k.calls);
    }
    // The window passes: bob is let back in.
    k.words = until;
    assert!(matches!(call(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY)), Ok(Reply::Login(_))));
    // The vault arm (R40): alice's vault session, logged in above, outlives three blames on her
    // unlabelled domain, which end her unlabelled session alone; her vault login is admitted
    // while her unlabelled one is locked out.
    k.calls.clear();
    for _ in 0..BLAME_COUNT {
        assert!(matches!(call(&mut s, &mut k, INIT, blame(1001, &[])), Ok(Reply::Blame(_))));
    }
    let destroyed: Vec<usize> =
        k.calls.iter().filter_map(|c| if let Call::Destroy(b) = c { Some(*b) } else { None }).collect();
    assert_eq!(destroyed, vec![alice_session_budget], "alice's unlabelled session alone");
    assert_eq!(call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)).unwrap_err(), ErrorCode::LockedOut);
    let vault = Message::Login(Login {
        principal: "alice",
        label: "alice-secrets",
        context: "v",
        from: "",
        key: &ALICE_KEY,
    });
    assert!(matches!(call(&mut s, &mut k, SSHD, vault), Ok(Reply::Login(_))));
    assert!(s.failed.is_empty());
}
/// A label set as the wire carries it.
fn label_bytes(ids: &[u64]) -> Vec<u8> { redoubt_rt::wire::labels::encode(ids).collect() }

fn submit<'a>(kind: u32, labels: &'a [u8], lease: u64, what: &'a str, reason: &'a str) -> Message<'a> {
    Message::Submit(Submit { kind, labels, lease, item: 0, source: 0, what, reason })
}

/// Opens an approval channel with `key`: the channel's id.
fn open_channel(s: &mut Steward<usize, usize>, k: &mut Recorder, key: &[u8; 32]) -> Result<u64, ErrorCode> {
    match call(s, k, SSHD, Message::ApprovalOpened(ApprovalOpened { key }))? {
        Reply::ApprovalOpened(r) => Ok(r.channel),
        other => panic!("{other:?}"),
    }
}

/// The screens written on `channel`, in order.
fn screens(k: &Recorder, channel: u64) -> Vec<String> {
    k.calls
        .iter()
        .filter_map(|c| match c {
            Call::Screen { channel: ch, text } if *ch == channel => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// The hash and the code the last screen on `channel` showed for `request`.
fn shown(k: &Recorder, channel: u64, request: u64) -> ([u8; 32], Vec<u8>) {
    let last = screens(k, channel).pop().expect("a screen");
    let line =
        last.lines().find(|l| l.starts_with(&format!("request {request:016x} hash "))).expect("the request");
    let hex = &line["request 0000000000000000 hash ".len()..][..64];
    let mut hash = [0; 32];
    for (i, b) in hash.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
    }
    assert!(line.ends_with(&format!(" code {}", &hex[..8])), "{line}");
    (hash, hash[..4].to_vec())
}

/// `approve@box` (servers/steward.md, "The powerbox and approvals", R38): a channel opens on one
/// of the principal's approval keys alone, the principal found from the key, and is redrawn at
/// once; a login key is `bad_key` (the core's guard, told the key's owner), a key of nobody's
/// `unknown`; past the server's readers the open fails and the core is told the channel closed;
/// `sshd`'s close ends it.
#[test]
fn an_approval_channel_opens_on_the_approval_key_alone() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    assert_eq!(open_channel(&mut s, &mut k, &ALICE_KEY).unwrap_err(), ErrorCode::BadKey);
    assert_eq!(open_channel(&mut s, &mut k, &[5; 32]).unwrap_err(), ErrorCode::Unknown);
    assert_eq!(k.calls, [Call::ApprovalConsoles(Some(CONSOLE)), Call::ApprovalConsoles(Some(CONSOLE))]);
    k.calls.clear();
    let c = open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap();
    assert!(c >= MINTED);
    assert_eq!(
        k.calls,
        [
            Call::ApprovalConsoles(Some(CONSOLE)),
            Call::ChannelOpened(c),
            Call::Screen { channel: c, text: "no approval is waiting for alice\r\n".into() },
        ]
    );
    k.calls.clear();
    k.channels_full = true;
    assert_eq!(open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap_err(), ErrorCode::Failed);
    assert!(matches!(k.calls[..], [Call::ApprovalConsoles(_), Call::ChannelOpened(_)]), "{:?}", k.calls);
    k.channels_full = false;
    k.calls.clear();
    // The first channel is still open: a `pending` on it redraws; one on a channel nobody holds
    // is unknown.
    assert!(matches!(
        call(&mut s, &mut k, APPROVAL, Message::Pending(Pending { channel: c })),
        Ok(Reply::Pending(_))
    ));
    assert_eq!(screens(&k, c), ["no approval is waiting for alice\r\n"]);
    let e = call(&mut s, &mut k, APPROVAL, Message::Pending(Pending { channel: c + 1 })).unwrap_err();
    assert_eq!(e, ErrorCode::Unknown);
    k.calls.clear();
    let closed = call(&mut s, &mut k, SSHD, Message::ApprovalClosed(ApprovalClosed { channel: c }));
    assert!(matches!(closed, Ok(Reply::ApprovalClosed(_))));
    assert_eq!(k.calls, [Call::ChannelClosed(c)]);
    let e = call(&mut s, &mut k, APPROVAL, Message::Pending(Pending { channel: c })).unwrap_err();
    assert_eq!(e, ErrorCode::Unknown);
}

/// The request and approval path (servers/steward.md, "The powerbox and approvals"; R38's
/// binding): alice's session submits a request for an agent in her vault's label set; her
/// session is told an approval is waiting, and the channel redrawn with the request's id, its
/// binding hash and the hash's code beside the core's rendered line; an approval naming another
/// hash, or a code the screen did not show, is `hash_mismatch` to the channel and starts
/// nothing; the code the screen showed approves it, and the lease batch runs in the labelled
/// domain: its budget from the `{7}` sub-budget with the label, its connections a labelled
/// lease's; the approved request is off the next screen.
#[test]
fn a_request_is_rendered_bound_to_its_screen_and_its_approval_starts_the_labelled_agent() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let c = open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap();
    k.calls.clear();
    let labels = label_bytes(&[7]);
    let Ok(Reply::Submit(r)) = call(&mut s, &mut k, u, submit(1, &labels, 60_000_000, "", "audit the vault"))
    else {
        panic!()
    };
    assert!(r.request >= MINTED);
    assert!(
        k.calls.iter().any(|c| matches!(c, Call::NotifyApproval(_))),
        "her session is told: {:?}",
        k.calls
    );
    let screen = screens(&k, c).pop().expect("the channel is redrawn");
    assert!(screen.starts_with("pending approvals for alice:\r\n"), "{screen}");
    assert!(screen.contains(&format!("request {:016x} hash ", r.request)), "{screen}");
    assert!(
        screen.contains("start an agent labelled [7] for 1 min; reason (untrusted): \"audit the vault\""),
        "{screen}"
    );
    assert!(screen.ends_with("type: approve <id> <hash or code> | deny <id> | pending\r\n"), "{screen}");
    let (hash, code) = shown(&k, c, r.request);
    k.calls.clear();
    let wrong = Message::Approve(Approve { channel: c, request: r.request, hash: &[0; 32] });
    assert_eq!(call(&mut s, &mut k, APPROVAL, wrong).unwrap_err(), ErrorCode::HashMismatch);
    let mut other = code.clone();
    other[0] ^= 1;
    let wrong_code = Message::Approve(Approve { channel: c, request: r.request, hash: &other });
    assert_eq!(call(&mut s, &mut k, APPROVAL, wrong_code).unwrap_err(), ErrorCode::HashMismatch);
    let short = Message::Approve(Approve { channel: c, request: r.request, hash: &hash[..3] });
    assert_eq!(call(&mut s, &mut k, APPROVAL, short).unwrap_err(), ErrorCode::HashMismatch);
    let elsewhere = Message::Approve(Approve { channel: c + 1, request: r.request, hash: &code });
    assert_eq!(call(&mut s, &mut k, APPROVAL, elsewhere).unwrap_err(), ErrorCode::Unknown);
    assert!(k.calls.is_empty(), "nothing started: {:?}", k.calls);
    let approve = Message::Approve(Approve { channel: c, request: r.request, hash: &code });
    assert!(matches!(call(&mut s, &mut k, APPROVAL, approve), Ok(Reply::Approve(_))));
    let alice_secrets = 3;
    assert_eq!(
        k.calls[0],
        Call::Create { parent: alice_secrets, pages: 10, processes: 2, labels: vec![7], account: 0 }
    );
    let connects: Vec<Call> = k.calls.iter().filter(|c| matches!(c, Call::Connect { .. })).cloned().collect();
    assert_eq!(connects.len(), 6);
    assert!(
        connects
            .iter()
            .all(|c| matches!(c, Call::Connect { kind: Kind::Lease, labels, .. } if labels == &[7]))
    );
    match k.calls.iter().find(|c| matches!(c, Call::Launch { .. })) {
        Some(Call::Launch { kind: Kind::Lease, connections, .. }) => {
            let bound: Vec<bool> = connections.iter().map(Option::is_some).collect();
            assert_eq!(
                bound,
                [true, true, true, false, false, false, true],
                "the steward's, /boot, the vault at /project, slot 5"
            );
        }
        other => panic!("{other:?}"),
    }
    // Approved is final: a second approval names nothing pending, and the screen is empty.
    let again = Message::Approve(Approve { channel: c, request: r.request, hash: &hash });
    assert_eq!(call(&mut s, &mut k, APPROVAL, again).unwrap_err(), ErrorCode::Unknown);
    k.calls.clear();
    let _ = call(&mut s, &mut k, APPROVAL, Message::Pending(Pending { channel: c }));
    assert_eq!(screens(&k, c), ["no approval is waiting for alice\r\n"]);
}

/// A note is rendered quoted and marked untrusted from an unlabelled session, and withheld from a
/// labelled one (R38: a labelled requester's free text would carry the vault out); a denial
/// takes the request off the screen; a session's end drops its requests, so the next screen
/// lists none of them. A declassification is STEWARD4's: `unknown` until then.
#[test]
fn notes_are_rendered_by_their_requesters_labels_and_a_dead_sessions_requests_are_gone() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let Ok(Reply::Login(v)) = call(&mut s, &mut k, SSHD, login("alice", "alice-secrets", &ALICE_KEY)) else {
        panic!()
    };
    let vault = badge_of(&s, v.session);
    let c = open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap();
    k.calls.clear();
    let Ok(Reply::Submit(plain)) = call(&mut s, &mut k, u, submit(0, &[], 0, "ship it \x1b[31m", "please"))
    else {
        panic!()
    };
    let screen = screens(&k, c).pop().unwrap();
    assert!(
        screen.contains("a note (untrusted): \"ship it ?[31m\"; reason (untrusted): \"please\""),
        "{screen}"
    );
    k.calls.clear();
    let Ok(Reply::Submit(secret)) =
        call(&mut s, &mut k, vault, submit(0, &[], 0, "the secret", "the secret"))
    else {
        panic!()
    };
    let screen = screens(&k, c).pop().unwrap();
    assert!(
        screen.contains("a note (text withheld: labelled requester); reason withheld (labelled requester)"),
        "{screen}"
    );
    assert!(!screen.contains("the secret"), "{screen}");
    assert!(screen.contains(&format!("request {:016x} hash ", plain.request)), "both pending: {screen}");
    let (_, secret_code) = shown(&k, c, secret.request);
    let e = call(&mut s, &mut k, u, submit(2, &label_bytes(&[7]), 0, "", "")).unwrap_err();
    assert_eq!(e, ErrorCode::Unknown, "a declassification is not bound yet");
    k.calls.clear();
    let deny = Message::Deny(Deny { channel: c, request: secret.request });
    assert!(matches!(call(&mut s, &mut k, APPROVAL, deny), Ok(Reply::Deny(_))));
    let _ = call(&mut s, &mut k, APPROVAL, Message::Pending(Pending { channel: c }));
    let screen = screens(&k, c).pop().unwrap();
    assert!(screen.contains(&format!("request {:016x}", plain.request)), "{screen}");
    assert!(!screen.contains(&format!("request {:016x}", secret.request)), "{screen}");
    // The denied request's code was on an earlier screen, not the last: an approval by it is a
    // mismatch to the channel, and nothing of a final request moves.
    let stale = Message::Approve(Approve { channel: c, request: secret.request, hash: &secret_code });
    assert_eq!(call(&mut s, &mut k, APPROVAL, stale).unwrap_err(), ErrorCode::HashMismatch);
    k.calls.clear();
    assert!(matches!(call(&mut s, &mut k, u, Message::EndSession(EndSession {})), Ok(Reply::EndSession(_))));
    k.calls.clear();
    let _ = call(&mut s, &mut k, APPROVAL, Message::Pending(Pending { channel: c }));
    assert_eq!(screens(&k, c), ["no approval is waiting for alice\r\n"]);
}

// ---- declassification and push ----

/// A vault session's badge: alice in `{alice-secrets}`.
fn vault_session(s: &mut Steward<usize, usize>, k: &mut Recorder) -> u64 {
    let Ok(Reply::Login(r)) = call(s, k, SSHD, login("alice", "alice-secrets", &ALICE_KEY)) else { panic!() };
    let badge = badge_of(s, r.session);
    k.calls.clear();
    badge
}

/// The budget the crossing program ran in, by its `Cross` call.
fn crossing_budget(calls: &[Call]) -> usize {
    calls
        .iter()
        .find_map(|c| match c {
            Call::Cross { budget, .. } => Some(*budget),
            _ => None,
        })
        .expect("a crossing")
}

/// The `Create`, `Cross`, `Destroy` and `Release` calls among `calls`, in order: a crossing's
/// batch and its cleanup, the rest (the screen, the notices) left out.
fn crossing_calls(calls: &[Call]) -> Vec<Call> {
    calls
        .iter()
        .filter(|c| {
            matches!(c, Call::Create { .. } | Call::Cross { .. } | Call::Destroy(_) | Call::Release(_))
        })
        .cloned()
        .collect()
}

/// A declassification (servers/steward.md, "Declassification and push"; R42): the vault
/// session's `submit` opens a read crossing, a budget under `users` with exactly the item's
/// labels and the domain's account, the `crossing` program's one read through it, the budget
/// destroyed and its connection given back after; the screen shows the whole item with the path
/// as the requester's text; the approval copies exactly the snapshot to `~/out/<name>`, by the
/// steward's own write and no read of the vault, audited with the reader's budget; the item's
/// path is forgotten with the request.
#[test]
fn a_declassification_reads_through_a_crossing_and_its_approval_copies_the_snapshot_out() {
    let mut k = Recorder::default();
    k.vault.insert("notes/memo".into(), b"forty bytes of text, printable, as asked".to_vec());
    let mut s = started(&mut k);
    let v = vault_session(&mut s, &mut k);
    let c = open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap();
    k.calls.clear();
    let held = s.holding();
    let labels = label_bytes(&[7]);
    let (reply, outputs) = call_outputs(&mut s, &mut k, v, submit(2, &labels, 0, "notes/memo", ""));
    let Ok(Reply::Submit(r)) = reply else { panic!("{reply:?}") };
    assert_eq!(records(&outputs), [Record::Submitted { request: r.request }]);
    assert_eq!(k.calls[0], Call::HomeStat { principal: "alice".into(), path: "out/memo".into() });
    let budget = crossing_budget(&k.calls);
    assert_eq!(
        crossing_calls(&k.calls),
        [
            Call::Create { parent: USERS, pages: 2, processes: 1, labels: vec![7], account: 1001 },
            Call::Cross { budget, labels: vec![7], path: "notes/memo".into(), write: None },
            Call::Destroy(budget),
            Call::Release(budget + 1),
        ],
        "{:?}",
        k.calls
    );
    assert!(!k.calls.iter().any(|c| matches!(c, Call::HomeRead { .. })), "the steward never reads the vault");
    let screen = screens(&k, c).pop().expect("the channel is redrawn");
    assert!(
        screen.contains("declassify item ")
            && screen.contains("of labels [7]: \"forty bytes of text, printable, as asked\""),
        "{screen}"
    );
    assert!(screen.contains(": path (the requester's text) \"notes/memo\"\r\n"), "{screen}");
    assert_eq!(s.items_held(), 1);
    // The item changes after the submission: what lands is the snapshot (R42).
    k.vault.insert("notes/memo".into(), b"changed since".to_vec());
    let (_, code) = shown(&k, c, r.request);
    k.calls.clear();
    let approve = Message::Approve(Approve { channel: c, request: r.request, hash: &code });
    let (reply, outputs) = call_outputs(&mut s, &mut k, APPROVAL, approve);
    assert!(matches!(reply, Ok(Reply::Approve(_))), "{reply:?}");
    assert_eq!(
        k.calls.iter().filter(|c| !matches!(c, Call::Screen { .. })).cloned().collect::<Vec<_>>(),
        [Call::HomeWrite {
            principal: "alice".into(),
            path: "out/memo".into(),
            bytes: b"forty bytes of text, printable, as asked".to_vec(),
        }],
        "the copy out is the steward's own write of the snapshot, and nothing else"
    );
    let reader = budget as u64;
    assert!(
        records(&outputs).iter().any(|x| matches!(x, Record::Declassified { request, bytes, reader: b } if *request == r.request && bytes == b"forty bytes of text, printable, as asked" && *b == reader)),
        "{:?}",
        records(&outputs)
    );
    assert_eq!(
        k.homes.get(&("alice".into(), "out/memo".into())).map(Vec::as_slice),
        Some(&b"forty bytes of text, printable, as asked"[..])
    );
    assert_eq!(s.items_held(), 0, "the item's path goes with the request");
    assert_eq!(s.holding(), held, "nothing of the crossing is held");
    // The landing is taken now: a second declassification of the same name is `in_use`, before
    // any budget is carved.
    k.calls.clear();
    let again = call(&mut s, &mut k, v, submit(2, &labels, 0, "notes/memo", ""));
    assert_eq!(again.unwrap_err(), ErrorCode::InUse);
    assert!(crossing_calls(&k.calls).is_empty(), "{:?}", k.calls);
    assert_eq!(s.items_held(), 0);
}

/// An item over `DECLASSIFY_MAX` is `too_big` and one that is not printable text
/// `not_printable`, each at submission, by the core's `item_fits` on the snapshot the crossing
/// read; the crossing is destroyed and nothing is kept.
#[test]
fn an_item_too_big_or_not_printable_is_refused_at_submission_and_the_crossing_destroyed() {
    let mut k = Recorder::default();
    k.vault.insert("big".into(), vec![b'x'; 300]);
    k.vault.insert("binary".into(), vec![0x01, 0x02, b'a']);
    let mut s = started(&mut k);
    let v = vault_session(&mut s, &mut k);
    let held = s.holding();
    let labels = label_bytes(&[7]);
    for (path, code) in [("big", ErrorCode::TooBig), ("binary", ErrorCode::NotPrintable)] {
        k.calls.clear();
        assert_eq!(call(&mut s, &mut k, v, submit(2, &labels, 0, path, "")).unwrap_err(), code);
        let calls = crossing_calls(&k.calls);
        assert!(
            matches!(
                calls.as_slice(),
                [Call::Create { .. }, Call::Cross { .. }, Call::Destroy(_), Call::Release(_)]
            ),
            "{calls:?}"
        );
        assert_eq!(s.items_held(), 0, "{path}");
        assert_eq!(s.holding(), held, "{path}");
    }
    // A path that does not clean (an empty one, the root, a name the path grammar refuses) names
    // nothing, and an unlabelled session declassifies nothing.
    for bad in ["", "/", ".", "a\u{0}b"] {
        assert_eq!(
            call(&mut s, &mut k, v, submit(2, &labels, 0, bad, "")).unwrap_err(),
            ErrorCode::Unknown,
            "{bad:?}"
        );
    }
    let u = alice_session(&mut s, &mut k);
    assert_eq!(call(&mut s, &mut k, u, submit(2, &labels, 0, "big", "")).unwrap_err(), ErrorCode::NotOwner);
}

/// A reader that dies (the crossing program's call fails): the crossing runs `Failed`, then
/// `destroy_partial` in `Closing`, and the submission is `failed`; nothing of it is held.
#[test]
fn a_reader_that_dies_fails_the_submission_and_the_partial_crossing_is_destroyed() {
    let mut k = Recorder { cross_fails: Some(Error::Dead), ..Recorder::default() };
    let mut s = started(&mut k);
    let v = vault_session(&mut s, &mut k);
    let held = s.holding();
    let labels = label_bytes(&[7]);
    assert_eq!(call(&mut s, &mut k, v, submit(2, &labels, 0, "memo", "")).unwrap_err(), ErrorCode::Failed);
    let budget = crossing_budget(&k.calls);
    assert_eq!(
        crossing_calls(&k.calls),
        [
            Call::Create { parent: USERS, pages: 2, processes: 1, labels: vec![7], account: 1001 },
            Call::Cross { budget, labels: vec![7], path: "memo".into(), write: None },
            Call::Destroy(budget),
        ]
    );
    assert_eq!(s.failed.len(), 1, "the failed step is said once: {:?}", s.failed);
    assert_eq!(s.items_held(), 0);
    assert_eq!(s.holding(), held);
}

/// A push (servers/steward.md, "Declassification and push"; R42): the unlabelled session's
/// `submit` has the steward read the source itself, from the submitter's home; the screen shows
/// the source and target by id with their paths, the size and the digest, never the bytes; the
/// approval opens a write crossing in the target's domain, exactly the target's labels and the
/// account, the program's one write of the snapshot, the budget destroyed and its connection
/// given back; audited `Pushed` in the target's domain. A vault session cannot push, another
/// principal cannot push into alice's labels, a source over `PUSH_MAX` is `too_big` and a missing
/// one `unknown`, each before any budget is carved.
#[test]
fn a_push_reads_the_source_unlabelled_and_its_approval_writes_through_a_crossing() {
    let mut k = Recorder::default();
    let bytes = vec![0xA5u8; 4096];
    k.homes.insert(("alice".into(), "in".into()), bytes.clone());
    k.homes.insert(("alice".into(), "huge".into()), vec![0u8; redoubt_steward_server::PUSH_MAX + 1]);
    k.homes.insert(("bob".into(), "in".into()), bytes.clone());
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let v = vault_session(&mut s, &mut k);
    let c = open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap();
    let labels = label_bytes(&[7]);
    // The refusals first: none carves a budget or reads anything.
    k.calls.clear();
    assert_eq!(
        call(&mut s, &mut k, v, submit(3, &labels, 0, "in\nsecret/in", "")).unwrap_err(),
        ErrorCode::NotOwner
    );
    assert_eq!(
        call(&mut s, &mut k, u, submit(3, &labels, 0, "huge\nsecret/in", "")).unwrap_err(),
        ErrorCode::TooBig
    );
    assert_eq!(
        call(&mut s, &mut k, u, submit(3, &labels, 0, "missing\nsecret/in", "")).unwrap_err(),
        ErrorCode::Unknown
    );
    assert_eq!(call(&mut s, &mut k, u, submit(3, &labels, 0, "in", "")).unwrap_err(), ErrorCode::Unknown);
    let Ok(Reply::Login(b)) = call(&mut s, &mut k, SSHD, bob_in("")) else { panic!() };
    let bob = badge_of(&s, b.session);
    assert_eq!(
        call(&mut s, &mut k, bob, submit(3, &labels, 0, "in\nsecret/in", "")).unwrap_err(),
        ErrorCode::NotOwner
    );
    assert!(
        crossing_calls(&k.calls)
            .iter()
            .all(|c| !matches!(c, Call::Create { parent: USERS, .. } | Call::Cross { .. })),
        "{:?}",
        k.calls
    );
    assert_eq!(s.items_held(), 0);
    k.calls.clear();
    let (reply, outputs) = call_outputs(&mut s, &mut k, u, submit(3, &labels, 0, "in\nsecret/in", "a push"));
    let Ok(Reply::Submit(r)) = reply else { panic!("{reply:?}") };
    assert_eq!(records(&outputs), [Record::Submitted { request: r.request }]);
    assert_eq!(
        k.calls
            .iter()
            .filter(|c| !matches!(c, Call::Screen { .. } | Call::NotifyApproval(_)))
            .cloned()
            .collect::<Vec<_>>(),
        [
            Call::HomeStat { principal: "alice".into(), path: "in".into() },
            Call::HomeRead { principal: "alice".into(), path: "in".into() },
        ],
        "the steward reads the source itself, and nothing crosses yet"
    );
    let screen = screens(&k, c).pop().expect("the channel is redrawn");
    assert!(screen.contains("push unlabelled item ") && screen.contains(" (4096 bytes, sha256 "), "{screen}");
    assert!(screen.contains(" of labels [7]; reason (untrusted): \"a push\""), "{screen}");
    assert!(screen.contains(": path (the requester's text) \"in\"\r\n"), "{screen}");
    assert!(screen.contains(": path (the requester's text) \"secret/in\"\r\n"), "{screen}");
    assert!(!screen.contains("\u{a5}"), "no bytes on the screen");
    assert_eq!(s.items_held(), 2);
    let (_, code) = shown(&k, c, r.request);
    k.calls.clear();
    let approve = Message::Approve(Approve { channel: c, request: r.request, hash: &code });
    let (reply, outputs) = call_outputs(&mut s, &mut k, APPROVAL, approve);
    assert!(matches!(reply, Ok(Reply::Approve(_))), "{reply:?}");
    let budget = crossing_budget(&k.calls);
    assert_eq!(
        crossing_calls(&k.calls),
        [
            Call::Create { parent: USERS, pages: 2, processes: 1, labels: vec![7], account: 1001 },
            Call::Cross { budget, labels: vec![7], path: "secret/in".into(), write: Some(bytes.clone()) },
            Call::Destroy(budget),
            Call::Release(budget + 1),
        ],
        "{:?}",
        k.calls
    );
    assert_eq!(k.vault.get("secret/in"), Some(&bytes));
    let pushed = records(&outputs).into_iter().find(|r| matches!(r, Record::Pushed { .. })).expect("Pushed");
    assert!(
        matches!(&pushed, Record::Pushed { request, bytes: b, writer, .. } if *request == r.request && *b == bytes && *writer == budget as u64),
        "{pushed:?}"
    );
    let domain = outputs
        .iter()
        .find_map(|o| match o {
            Output::Audit(a) if matches!(a.record(), Record::Pushed { .. }) => Some(a.domain().clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(domain.labels().as_slice(), [7], "audited in the target's domain");
    assert_eq!(s.items_held(), 0);
}

/// A writer that fails after some bytes: the crossing runs `Failed`, `audit_push_failed`, then
/// `destroy_partial`; the vault is left as it is, which the page states (the next push
/// overwrites the partial file).
#[test]
fn a_writer_that_fails_is_audited_and_the_partial_crossing_destroyed() {
    let mut k = Recorder::default();
    k.homes.insert(("alice".into(), "in".into()), vec![1u8; 16]);
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let c = open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap();
    let labels = label_bytes(&[7]);
    let Ok(Reply::Submit(r)) = call(&mut s, &mut k, u, submit(3, &labels, 0, "in\nin", "")) else { panic!() };
    let (_, code) = shown(&k, c, r.request);
    let held = s.holding();
    k.cross_fails = Some(Error::Timeout);
    k.calls.clear();
    let approve = Message::Approve(Approve { channel: c, request: r.request, hash: &code });
    let (reply, outputs) = call_outputs(&mut s, &mut k, APPROVAL, approve);
    assert!(matches!(reply, Ok(Reply::Approve(_))), "the approval stands; the crossing failed: {reply:?}");
    assert!(
        matches!(
            crossing_calls(&k.calls).as_slice(),
            [Call::Create { .. }, Call::Cross { write: Some(_), .. }, Call::Destroy(_)]
        ),
        "{:?}",
        k.calls
    );
    assert!(
        records(&outputs)
            .iter()
            .any(|x| matches!(x, Record::PushFailed { request } if *request == r.request)),
        "{:?}",
        records(&outputs)
    );
    assert_eq!(s.items_held(), 0);
    assert_eq!(s.holding(), held);
}

/// A denied or dropped request forgets its items too.
#[test]
fn a_denied_requests_items_are_forgotten() {
    let mut k = Recorder::default();
    k.homes.insert(("alice".into(), "in".into()), vec![1u8; 16]);
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let c = open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap();
    let labels = label_bytes(&[7]);
    let Ok(Reply::Submit(r)) = call(&mut s, &mut k, u, submit(3, &labels, 0, "in\nin", "")) else { panic!() };
    assert_eq!(s.items_held(), 2);
    let deny = Message::Deny(Deny { channel: c, request: r.request });
    assert!(matches!(call(&mut s, &mut k, APPROVAL, deny), Ok(Reply::Deny(_))));
    assert_eq!(s.items_held(), 0);
    let Ok(Reply::Submit(_)) = call(&mut s, &mut k, u, submit(3, &labels, 0, "in\nin", "")) else { panic!() };
    assert_eq!(s.items_held(), 2);
    assert!(matches!(call(&mut s, &mut k, u, Message::EndSession(EndSession {})), Ok(Reply::EndSession(_))));
    assert_eq!(s.items_held(), 0, "a dead session's requests are dropped, their items with them");
}

/// In a confined boot (servers/init.md, "The confinement check"; R34) a labelled session's home
/// slot binds nothing and its VM is told no home: a push is its way in; an unlabelled session's
/// home is bound as ever.
#[test]
fn a_confined_boot_leaves_a_labelled_sessions_home_unbound() {
    use redoubt_steward_server::own::{Bound, Child, Own, binding, session_args};
    let mut own = Own::default();
    own.line("label \"alice-secrets\" id=7").unwrap();
    own.line("home \"alice\" handle=walfsd:data path=/home/alice quota=8388608").unwrap();
    own.line("vault \"alice\" labels=[7] handle=littlefsd:alice-secrets").unwrap();
    let home = |own: &Own, child| binding(own, "alice", &[7], child, 1, 0).map(|b: Bound| b.at);
    assert_eq!(home(&own, Child::Session { labelled: true }), Some(Some("/home/alice".into())));
    assert!(session_args(&own, "alice", &[7], None).contains(&"home=/home/alice".to_string()));
    own.line("confined").unwrap();
    assert!(own.confined);
    assert_eq!(home(&own, Child::Session { labelled: true }), None);
    assert!(!session_args(&own, "alice", &[7], None).iter().any(|a| a.starts_with("home=")));
    assert_eq!(
        binding(&own, "alice", &[], Child::Session { labelled: false }, 1, 0).map(|b| b.at),
        Some(Some("/home/alice".into()))
    );
    assert!(session_args(&own, "alice", &[], None).contains(&"home=/home/alice".to_string()));
    assert_eq!(own.line("confined"), Err("a second `confined`".into()));
    assert_eq!(Own::default().line("confined now"), Err("`confined` stands alone".into()));
}
