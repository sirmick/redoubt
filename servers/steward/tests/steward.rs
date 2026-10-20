//! The steward against a kernel that records its calls (servers/steward.md, "Principals",
//! "Fixed sub-budgets per label set", "Authentication and sessions" and "The steward's
//! protocol"): its start from the manifest lines, and each call through the protocol, the core
//! and the batches it names.

use std::collections::{BTreeMap, BTreeSet};

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
use redoubt_steward::effect::{Batch, Kind, Object, Output, Step, Token};
use redoubt_steward::effects::SCOPE;
use redoubt_steward::event::EventKind;
use redoubt_steward::hash::key_id;
use redoubt_steward_server::cleanup::{Fate, Queue as Cleanup, RETRY_US, TRIES};
use redoubt_steward_server::console::{Queue, Sink};
use redoubt_steward_server::drive::{MINTED, Ran};
use redoubt_steward_server::own::{Child, binds};
use redoubt_steward_server::protocol::{APPROVAL, INIT, SSHD, Serving, answer_late, answer_with, watches};
use redoubt_steward_server::{Kernel, Launched, SLOTS, StartError, Steward, start};

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
    /// A connection handed to the cleanup thread by the loop: no call to its server.
    Hand(usize),
    /// A connection's server told, by the cleanup thread: `parks` on the attempt past which the
    /// step waits for the server's next answer.
    Clean {
        handle: usize,
        parks: bool,
    },
    /// The loop woke a server's parked steps (a mint there, answered or about to be made), or
    /// forgot a restarted server's.
    Wake(String),
    Forget(String),
    Launch {
        kind: Kind,
        budget: usize,
        connections: Vec<Option<usize>>,
        context: Option<String>,
    },
    /// The launch thread's report for a job, finished by the loop.
    Launched(u64),
    /// The install lock taken and released around the destroy pair.
    Locked,
    Unlocked,
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
    /// Each budget's parent.
    parents: BTreeMap<usize, usize>,
    /// The budgets live: made and not destroyed, by the steward or by the kernel's deadline.
    budgets: BTreeSet<usize>,
    /// The handle indices the kernel freed when a deadline took a budget
    /// ([`Recorder::deadline_takes`]): the next handle made takes the lowest, as the kernel's
    /// table does. (A destroy the steward makes frees its index on the machine too; not
    /// modelled: the steward drops the record as it destroys, so nothing of its looks that index
    /// up again, and the tests' handle numbers stay the kernel's order of making.)
    free: Vec<usize>,
    /// Budgets whose handle the steward lost before their deadline (a bug): a `destroy` of one is
    /// `BadHandle`.
    lost: Vec<usize>,
    /// The machine's handle table modelled whole, as the kernel keeps it: every copy the machine
    /// closes frees its index for the next handle made (the connections a launch hands its child,
    /// `launch`; a kept right given back, `release` and `clean`; the exit endpoint the loop closes
    /// at the notice, [`Recorder::exit_taken`]), and a destroy revokes the budget's own handle and
    /// its children's (`destroy`). Off, the tests' handle numbers stay the kernel's order of
    /// making, as [`Recorder::free`] says; on for the tests that look for a confusion across a
    /// reused index.
    recycles: bool,
    /// With `recycles`: the connections and rights the steward still holds a copy of.
    held: BTreeSet<usize>,
    /// With `recycles`: the budget the next `destroy` must hit, the one made for the lease whose
    /// step runs (the kernel's verdict); `None` once it did.
    expect_destroy: Option<usize>,
    /// Every reader thread is taken: the next channel cannot be served.
    channels_full: bool,
    /// What every cleanup step's server answers: `Ok` unless a test says (`Timeout`: the server
    /// did not take the call).
    clean: Option<Error>,
    /// The servers that take no cleanup step (`Timeout`) while named here, answering every
    /// other call.
    busy: Vec<String>,
    /// The cleanup queue the loop hands steps to and the cleanup thread runs
    /// ([`Recorder::thread`]): the machine shares it with its thread under a lock; here the
    /// thread runs when a test, or the loop's wait for its report, says.
    cleanup: Cleanup<usize>,
    /// The servers that take no call of the steward's within its bound (`SERVER_US`) while
    /// named here: a mint there, a launch whose image is read there (`bootfsd`), a home's stat
    /// or read there (`walfsd:data`) is `Timeout`.
    stalled: Vec<String>,
    /// Each connection made at a server, by the handle made, until the server is told it is
    /// gone: the server's records.
    at: BTreeMap<usize, String>,
    /// A server and the most records it admits: a connect there with as many is `Refused`.
    cap: Option<(String, usize)>,
    /// Microseconds the clock has been moved on by, beside the words drawn.
    clock: u64,
    /// The files in the principals' homes, by principal and path: what the steward itself reads
    /// and writes.
    homes: BTreeMap<(String, String), Vec<u8>>,
    /// The vault's items, by path: what a crossing's program reads, and where it writes.
    vault: BTreeMap<String, Vec<u8>>,
    /// The crossing program dies, times out or answers wrongly: its call fails with this.
    cross_fails: Option<Error>,
    /// Every launch is the launch thread's: `launch` hands it over as a job and the test
    /// reports it with `Steward::launched`, as the machine's loop does on the thread's report.
    launch_pends: bool,
    /// The jobs handed over so far, and the notices send right made for each still to report.
    jobs: u64,
    launching: BTreeMap<u64, (Option<usize>, Vec<usize>)>,
    /// What the thread reports for every launch: `Ok` unless a test says.
    launch_fails: Option<Error>,
    /// The install lock is held: the loop's destroy pair runs, and nothing installs.
    locked: bool,
}

impl Recorder {
    fn make(&mut self) -> Result<usize, Error> {
        assert!(!self.locked, "no handle is installed while the install lock is held for the destroy pair");
        if self.fail_at == Some(self.made) {
            return Err(Error::OutOfMemory);
        }
        if let Some(lowest) = self.free.iter().min().copied() {
            self.free.retain(|i| *i != lowest);
            return Ok(lowest);
        }
        self.made += 1;
        Ok(self.made)
    }

    /// The kernel's deadline takes `budget` (R10): it and every budget below it are destroyed,
    /// their handles revoked and the indices freed for the next handle made. Only once the
    /// clock has passed the deadline the budget was made with, as the kernel's expiry runs.
    fn deadline_takes(&mut self, budget: usize) {
        let (_, deadline) = self.deadlines.iter().find(|d| d.0 == budget).expect("a budget made");
        assert!(*deadline <= self.now(), "the kernel takes a budget at its deadline, never before");
        self.takes(vec![budget]);
    }

    /// The steward's destroy of `budget` took every budget below it too, as the kernel's does,
    /// their handles revoked and the indices freed for the next handle made; the budget's own
    /// index stays taken, as [`Recorder::free`] says.
    fn destroy_took(&mut self, budget: usize) {
        assert!(!self.budgets.contains(&budget), "budget {budget} was destroyed");
        let children = self.parents.iter().filter(|(_, p)| **p == budget).map(|(c, _)| *c).collect();
        self.takes(children);
    }

    /// Every budget in `doomed` and every budget below it leaves the live set, its index freed.
    fn takes(&mut self, mut doomed: Vec<usize>) {
        while let Some(b) = doomed.pop() {
            doomed.extend(self.parents.iter().filter(|(_, p)| **p == b).map(|(c, _)| *c));
            assert!(self.budgets.remove(&b), "budget {b} was live");
            self.free.push(b);
            // A freed index is the next handle's, whatever kind: its place in the tree goes.
            if self.recycles {
                self.parents.remove(&b);
            }
        }
    }

    /// With `recycles`: the loop closed the exit endpoint `pid` names, at its watcher's report,
    /// before the exit's event; its index is free for the next handle made.
    fn exit_taken(&mut self, pid: u64) {
        assert!(self.recycles);
        self.free.push(pid as usize);
    }

    /// With `recycles`: `handle` is a copy the steward holds; a close of it frees its index.
    fn holds(&mut self, handle: usize) {
        if self.recycles {
            self.held.insert(handle);
        }
    }

    /// With `recycles`: the steward's copy of `handle` goes, if it still held one.
    fn closes(&mut self, handle: usize) {
        if self.recycles && self.held.remove(&handle) {
            self.free.push(handle);
        }
    }

    fn creates(&self) -> Vec<&Call> {
        self.calls.iter().filter(|c| matches!(c, Call::Create { .. })).collect()
    }

    /// The image's binding table by handle name: `/boot` at `bootfsd`, the home at `walfsd:data`,
    /// the vault at `littlefsd:alice-secrets`, the network at `ipd`, the channel console `sshd`'s
    /// and the system volume at `erofsd:system`.
    fn server_of_slot(slot: u16) -> Option<&'static str> {
        Some(match slot {
            0 => "bootfsd",
            1 => "walfsd:data",
            2 => "littlefsd:alice-secrets",
            3 => "ipd",
            4 => "sshd",
            5 => "erofsd:system",
            _ => return None,
        })
    }

    /// The records `server` holds: the connections made there it was not yet told are gone.
    fn records(&self, server: &str) -> usize { self.at.values().filter(|s| *s == server).count() }

    /// A connection made at `server`, refused at its cap.
    fn made_at(&mut self, server: &str) -> Result<usize, Error> {
        if self.cap.as_ref().is_some_and(|(s, cap)| s == server && self.records(server) >= *cap) {
            return Err(Error::Refused);
        }
        let made = self.make()?;
        self.at.insert(made, server.into());
        Ok(made)
    }

    /// The cleanup thread's turn: every step due now, in order, each one call at its server
    /// (`Timeout` while the server is busy, else the record goes), settled as the queue says;
    /// the fates it reports to the loop (every one but a retry's). The machine's thread runs on
    /// its own and makes one call at a time; here it runs when a test says, so a test sees the
    /// loop's turn and the thread's apart.
    fn thread(&mut self) -> Vec<(usize, Fate)> {
        let now = self.now();
        let mut fates = Vec::new();
        while let Some((handle, tries)) = self.cleanup.take(now) {
            self.calls.push(Call::Clean { handle, parks: tries == TRIES });
            let told = self.tell(handle);
            if let Some(fate) = self.cleanup.settle(told, now).filter(|(_, f)| *f != Fate::Retry) {
                fates.push(fate);
            }
        }
        fates
    }

    /// A step's call at its server.
    fn tell(&mut self, handle: usize) -> Result<(), Error> {
        if let Some(e) = self.clean {
            return Err(e);
        }
        if self.at.get(&handle).is_some_and(|s| self.busy.contains(s)) {
            return Err(Error::Timeout);
        }
        self.at.remove(&handle);
        Ok(())
    }

    /// The connections handed over whose servers are still to be told, parked ones included.
    fn pending(&self) -> usize { self.cleanup.len() }

    /// How many of those are parked.
    fn parked(&self) -> usize { self.cleanup.parked() }
}

impl Kernel for Recorder {
    type Budget = usize;
    type Handle = usize;

    fn create(&mut self, parent: usize, spec: &BudgetSpec) -> Result<usize, Error> {
        let made = self.make()?;
        self.deadlines.push((made, spec.deadline));
        self.parents.insert(made, parent);
        self.budgets.insert(made);
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

    /// As the kernel answers a `budget_destroy` by index: a free index is `BadHandle`, an index
    /// holding no budget `WrongObject`, and a budget's index destroys that budget, whichever it
    /// is now.
    fn destroy(&mut self, budget: usize) -> Result<(), Error> {
        assert!(
            self.locked,
            "a destroy by a kept handle is made under the install lock, beside its clock read"
        );
        self.calls.push(Call::Destroy(budget));
        if self.free.contains(&budget) || self.lost.contains(&budget) {
            return Err(Error::BadHandle);
        }
        if self.recycles {
            // The index names what the kernel put there last: a budget, or another kind of
            // object (a connection, an exit endpoint).
            if !self.budgets.contains(&budget) {
                return Err(Error::WrongObject);
            }
            if let Some(expected) = self.expect_destroy.take() {
                assert_eq!(
                    budget, expected,
                    "the destroy by handle hit budget {budget}, not budget {expected}, the one made for the lease whose step this is"
                );
            }
            // R10: the budget and everything below it go, their handles revoked everywhere.
            self.takes(vec![budget]);
            return Ok(());
        }
        if !self.deadlines.iter().any(|d| d.0 == budget) {
            return Err(Error::WrongObject);
        }
        self.budgets.remove(&budget);
        Ok(())
    }

    fn budget_id(&self, budget: usize) -> u64 { budget as u64 }

    fn mint(&mut self, badge: u64, stamp: usize) -> Result<usize, Error> {
        let made = self.make()?;
        self.holds(made);
        self.calls.push(Call::Mint { badge, stamp });
        Ok(made)
    }

    /// The binding table's shape: slot 2 (the vault) binds to nothing unlabelled, slot 3 (the
    /// network) to nothing labelled; a lease binds its project root and the system volume alone.
    fn connect(&mut self, owner: &Object, slot: u16, scope: usize) -> Result<Option<usize>, Error> {
        let labels = owner.domain.labels().as_slice().to_vec();
        let binds = redoubt_steward_server::own::binds(Child::of(owner.kind, &labels), slot);
        self.calls.push(Call::Connect { kind: owner.kind, labels, slot, scope });
        if !binds {
            return Ok(None);
        }
        let made = match Self::server_of_slot(slot) {
            Some(server) if self.stalled.iter().any(|s| s == server) => return Err(Error::Timeout),
            Some(server) => self.made_at(server)?,
            None => self.make()?,
        };
        self.holds(made);
        Ok(Some(made))
    }

    fn console(&mut self, handle: Option<Handle>, relay: Option<Handle>) {
        assert_eq!(handle.is_some(), relay.is_some(), "a login brings both consoles or none");
        self.calls.push(Call::Console(handle.map(|h| h.index())));
    }

    fn release(&mut self, handle: usize) {
        self.calls.push(Call::Release(handle));
        self.closes(handle);
    }

    /// A connection with a server's record is a step of the thread's; one nobody is told (a
    /// badge, a notices send right, a relay's control connection) is done here.
    fn clean(&mut self, handle: usize) {
        self.calls.push(Call::Hand(handle));
        if let Some(server) = self.at.get(&handle).cloned() {
            let now = self.now();
            self.cleanup.push(handle, server, now);
        } else {
            self.closes(handle);
        }
    }

    fn parked_at(&self, server: &str) -> bool { self.cleanup.parked_at(server) > 0 }

    /// Recorded when it wakes something: with nothing parked there it is a look at the queue,
    /// as the machine's is.
    fn wake(&mut self, server: &str) {
        if self.cleanup.wake(server) {
            self.calls.push(Call::Wake(server.into()));
        }
    }

    fn forget(&mut self, server: &str) {
        self.calls.push(Call::Forget(server.into()));
        self.cleanup.forget(server);
    }

    fn server_at(&self, owner: &Object, slot: Option<u16>) -> Option<String> {
        let labels = owner.domain.labels().as_slice().to_vec();
        let Some(slot) = slot else { return Some("littlefsd:alice-secrets".into()) };
        if !binds(Child::of(owner.kind, &labels), slot) {
            return None;
        }
        Self::server_of_slot(slot).map(String::from)
    }

    /// Inline unless a test says the launches pend (`launch_pends`): then, as the machine's,
    /// the notices endpoint is made now and the process by [`Recorder::launched`] when the
    /// test says the thread reported.
    fn launch(
        &mut self,
        owner: &Object,
        budget: usize,
        connections: &[Option<usize>],
        context: Option<&str>,
    ) -> Result<Launched<usize>, Error> {
        // A session's VM is given a notices endpoint; the handle made is its send right. Made
        // before the process, so the last thing a login makes is its pid.
        let notices = if owner.kind == Kind::Session { Some(self.make()?) } else { None };
        if let Some(n) = notices {
            self.holds(n);
        }
        let context = context.map(String::from);
        self.calls.push(Call::Launch {
            kind: owner.kind,
            budget,
            connections: connections.to_vec(),
            context,
        });
        let given: Vec<usize> = connections.iter().flatten().copied().collect();
        if self.launch_pends {
            self.jobs += 1;
            self.launching.insert(self.jobs, (notices, given));
            return Ok(Launched::Pending(self.jobs));
        }
        // The image is read from `bootfsd` before anything of the process exists.
        if self.stalled.iter().any(|s| s == "bootfsd") {
            return Err(Error::Timeout);
        }
        // The pid the machine answers is the exit endpoint's index, the handle made for it.
        let pid = self.make()? as u64;
        // The child holds its copies now: the steward's go (`bin/steward.rs`, `launched`).
        for c in given {
            self.closes(c);
        }
        Ok(Launched::Started(pid, notices))
    }

    /// The thread's report for `job`: the process, or the launch failed as `launch_fails` says
    /// (the budget gone under it, a read the server did not take), the notices send right
    /// closed with it; the steward's copies of what the child was given go as it starts.
    fn launched(&mut self, job: u64) -> Result<(u64, Option<usize>), Error> {
        let (notices, given) = self.launching.remove(&job).expect("a job the loop handed over");
        self.calls.push(Call::Launched(job));
        if let Some(e) = self.launch_fails {
            if let Some(n) = notices {
                self.closes(n);
            }
            return Err(e);
        }
        let pid = self.make()? as u64;
        for c in given {
            self.closes(c);
        }
        Ok((pid, notices))
    }

    /// Recorded as a bracket, so a test sees what ran inside it: the destroy pair and nothing
    /// that installs a handle ([`Recorder::make`] refuses one under the lock).
    fn without_installs<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        assert!(!self.locked, "the install lock is not re-entered");
        self.locked = true;
        self.calls.push(Call::Locked);
        let out = f(self);
        self.calls.push(Call::Unlocked);
        self.locked = false;
        out
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
        let made = self.made_at(Self::server_of_slot(slot).unwrap())?;
        Ok(Some((made, Handle::new(made as u32).unwrap(), at.into())))
    }

    fn launch_relay(&mut self, _domain: &Domain, budget: usize) -> Result<(u64, usize), Error> {
        let control = self.make()?;
        self.holds(control);
        self.calls.push(Call::LaunchRelay { budget, control });
        Ok((control as u64, control))
    }

    fn attach(&mut self, relay: usize, note: &str) -> Result<usize, Error> {
        let kept = self.made_at("sshd")?;
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
        let key = self.made_at("littlefsd:alice-secrets")?;
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
        if self.stalled.iter().any(|s| s == "walfsd:data") {
            return Err(Error::Timeout);
        }
        Ok(self.homes.get(&(principal.into(), path.into())).map(|b| b.len() as u64))
    }

    fn home_read(&mut self, principal: &str, path: &str, max: usize) -> Result<Vec<u8>, Error> {
        self.calls.push(Call::HomeRead { principal: principal.into(), path: path.into() });
        if self.stalled.iter().any(|s| s == "walfsd:data") {
            return Err(Error::Timeout);
        }
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
    (reply.map(owned), outputs)
}

/// An owned copy of what the tests read of a reply: a login's session and labels, a lease's name.
fn owned(r: Reply<'_>) -> Reply<'static> {
    match r {
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
    }
}

/// A call the server parks (`Serving::parked`): its event's reply is still to come, so nothing
/// is answered now. The call's caller, words and lend, for [`late`].
fn call_parked(
    s: &mut Steward<usize, usize>,
    k: &mut Recorder,
    badge: u64,
    m: Message<'_>,
    reply: u64,
) -> (Caller, redoubt_rt::ipc::Words, Vec<u8>) {
    let inline = matches!(m, Message::EndLease(_) | Message::ChannelClosed(_));
    let mut buf = if inline { Vec::new() } else { vec![0u8; 4096] };
    let words = m.encode(&mut buf).unwrap();
    let caller = Caller { badge, account: 0, labels: Default::default() };
    let mut handles = ReceivedHandles::new();
    if matches!(m, Message::Login(_)) {
        handles.push(Handle::new(CONSOLE)).unwrap();
        handles.push(Handle::new(CONSOLE + 1)).unwrap();
    }
    let mut serving = Serving::with_reply(s, k, reply);
    answer_with(&mut serving, &caller, &words, &handles, &mut buf);
    assert!(serving.parked, "the call is parked for its reply");
    assert!(serving.steward.awaits(reply));
    (caller, words, buf)
}

/// Answers a parked call from `outputs`, the core's reply on `reply` among them, as the loop does
/// on a launch's report: the reply, decoded.
fn late(
    s: &Steward<usize, usize>,
    parked: &(Caller, redoubt_rt::ipc::Words, Vec<u8>),
    outputs: &[Output],
    reply: u64,
) -> Result<Reply<'static>, ErrorCode> {
    let answer = outputs
        .iter()
        .find_map(|o| match o {
            Output::Reply { to, answer } if *to == reply => Some(answer.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a reply on {reply} among {outputs:?}"));
    let (caller, words, buf) = parked;
    let mut buf = buf.clone();
    let handles = if words[0] == 1 { 2 } else { 0 };
    let outcome = answer_late(s, caller, words, handles, &mut buf, answer);
    Reply::decode(words[0] as u32, &outcome.words, &buf, 0).unwrap().map(owned)
}

/// The id of the one lease of alice's that is still starting (its batch at its launch): the
/// caller does not know it yet, since its reply is to come.
fn starting_lease(s: &Steward<usize, usize>) -> u64 {
    redoubt_steward::inspect::domains(&s.store)
        .flat_map(|(_, d)| d.leases.values())
        .find(|l| l.state == redoubt_steward::gen::lease::State::Starting)
        .map(|l| l.id)
        .expect("a starting lease")
}

/// A launch is the launch thread's: `start_agent`'s batch stops at its `Launch` step with the
/// lease's budget, scope and connections made, the call is parked, and the lease is answered
/// when the thread's report is finished on the loop: the process kept, the batch's `Done` to
/// the core, the reply on the call's own token.
#[test]
fn a_launch_runs_on_the_thread_and_the_lease_is_answered_when_it_reports() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    k.launch_pends = true;
    let before = s.holding();
    let parked = call_parked(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 }), 3);
    assert_eq!(s.launching(), 1);
    assert!(matches!(k.calls.last(), Some(Call::Launch { kind: Kind::Lease, .. })), "{:?}", k.calls);
    assert!(s.failed.is_empty());
    let job = k.jobs;
    let outputs = s.launched(&mut k, job).unwrap();
    assert_eq!(k.calls.last(), Some(&Call::Launched(1)));
    let Ok(Reply::StartAgent(a)) = late(&s, &parked, &outputs, 3) else { panic!("{outputs:?}") };
    assert_eq!(a.name, "agent-1");
    assert!(s.exited(pid_of(&s, Kind::Lease)).is_some_and(|o| o.id == a.lease), "the process is kept");
    assert_eq!(s.launching(), 0);
    assert!(!s.awaits(3));
    assert_eq!(
        s.holding(),
        before + 10,
        "the budget, the scope, the steward's badge, a connection record per slot and the process"
    );
    // The outputs are the batch's records: the agent's start is audited once.
    assert_eq!(records(&outputs).iter().filter(|r| matches!(r, Record::AgentStarted { .. })).count(), 1);
}

/// While a launch waits on the thread, every other object is served: a deadline takes another
/// lease, and its exit is decided and the sponsor told before the launch reports; the idle
/// timer's event runs; another lease's `end_lease` is answered at once. Then the launch's
/// report answers its own call.
#[test]
fn a_deadline_exit_of_another_lease_is_decided_and_its_notice_sent_while_a_launch_waits() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let (first, budget, pid) = lease_for(&mut s, &mut k, u, 2_000_000);
    let (second, _, _) = lease_for(&mut s, &mut k, u, 60_000_000);
    k.launch_pends = true;
    let parked = call_parked(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 }), 3);
    k.clock += 2_000_000;
    k.deadline_takes(budget);
    k.calls.clear();
    let object = s.exited(pid).unwrap();
    s.event(&mut k, EventKind::Exited { object }, 0).unwrap();
    assert!(
        k.calls.iter().any(|c| matches!(c, Call::Notify { lease, .. } if *lease == first)),
        "{:?}",
        k.calls
    );
    assert!(s.failed.is_empty(), "{:?}", s.failed);
    s.event(&mut k, EventKind::Idle, 0).unwrap();
    k.calls.clear();
    let Ok(Reply::EndLease(_)) = call(&mut s, &mut k, u, Message::EndLease(EndLease { lease: second }))
    else {
        panic!()
    };
    assert!(
        k.calls.iter().any(|c| matches!(c, Call::Destroy(_))),
        "answered and done at once: {:?}",
        k.calls
    );
    assert_eq!(s.launching(), 1, "the launch still waits");
    let job = k.jobs;
    let outputs = s.launched(&mut k, job).unwrap();
    let Ok(Reply::StartAgent(a)) = late(&s, &parked, &outputs, 3) else { panic!("{outputs:?}") };
    assert_eq!(a.name, "agent-3");
}

/// The usage line a waiting launch defers is the carved-from domain's, not the owner's: a login
/// as the sponsored principal carves its session from alice's unlabelled sub-budget, so alice's
/// line waits for the launch's report (printed after the batch's `Login` record, the order the
/// cases match on) and agent's own domain, which has no sub-budget, is not what waits; once the
/// launch is reported nothing waits. On the loop-run launch every line followed its record.
#[test]
fn a_sponsored_logins_launch_defers_the_sponsors_usage_line_until_its_report() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    k.launch_pends = true;
    let alice = Domain::new(1001.try_into().unwrap(), redoubt_steward::domain::Labels::empty());
    let agent = Domain::new(1003.try_into().unwrap(), redoubt_steward::domain::Labels::empty());
    let parked = call_parked(&mut s, &mut k, SSHD, login("agent", "", &AGENT_KEY), 3);
    assert_eq!(s.launching(), 1);
    assert!(s.launching_in(&alice), "the sponsor's sub-budget is carved while the launch waits");
    assert!(!s.launching_in(&agent), "the sponsored principal has no sub-budget to say");
    let job = k.jobs;
    let outputs = s.launched(&mut k, job).unwrap();
    let Ok(Reply::Login(r)) = late(&s, &parked, &outputs, 3) else { panic!("{outputs:?}") };
    assert_eq!(r.name, "session-1");
    assert!(!s.launching_in(&alice));
    assert_eq!(records(&outputs).iter().filter(|r| matches!(r, Record::Login { .. })).count(), 1);
}

/// The launching lease's own `end_lease` (a sponsor that guessed its id) is held, its call
/// parked, and served after the launch's report, in order: the lease is answered started, then
/// ended and its sponsor told, as the two calls would have been served one after the other.
#[test]
fn the_launching_leases_own_end_lease_is_held_and_served_after_the_report() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    k.launch_pends = true;
    let start = call_parked(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 }), 3);
    let lease = starting_lease(&s);
    k.calls.clear();
    let end = call_parked(&mut s, &mut k, u, Message::EndLease(EndLease { lease }), 4);
    assert!(k.calls.is_empty(), "held: nothing ran for it: {:?}", k.calls);
    let job = k.jobs;
    let outputs = s.launched(&mut k, job).unwrap();
    let Ok(Reply::StartAgent(a)) = late(&s, &start, &outputs, 3) else { panic!("{outputs:?}") };
    assert_eq!(a.lease, lease);
    let Ok(Reply::EndLease(_)) = late(&s, &end, &outputs, 4) else { panic!("{outputs:?}") };
    let replies: Vec<u64> = outputs
        .iter()
        .filter_map(|o| if let Output::Reply { to, .. } = o { Some(*to) } else { None })
        .collect();
    assert_eq!(replies, [3, 4], "the start answered before the end");
    let launched = k.calls.iter().position(|c| matches!(c, Call::Launched(_))).unwrap();
    let destroyed = k.calls.iter().position(|c| matches!(c, Call::Destroy(_))).unwrap();
    assert!(launched < destroyed, "{:?}", k.calls);
    assert!(k.calls.iter().any(|c| matches!(c, Call::Notify { lease: l, .. } if *l == lease)));
    assert!(s.failed.is_empty() && s.launching() == 0 && !s.awaits(4));
}

/// A launch the thread fails (the budget gone under it, a read the server did not take) fails
/// the step as any step's failure does: `failed` to the caller, the core's rollback destroying
/// what the batch made, nothing of the lease held.
#[test]
fn a_launch_the_thread_fails_answers_failed_and_rolls_the_batch_back() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let before = s.holding();
    k.launch_pends = true;
    k.launch_fails = Some(Error::NotPermitted);
    let parked = call_parked(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 }), 3);
    let budget = leased(&k);
    k.calls.clear();
    let job = k.jobs;
    let outputs = s.launched(&mut k, job).unwrap();
    assert_eq!(late(&s, &parked, &outputs, 3).unwrap_err(), ErrorCode::Failed);
    assert!(matches!(s.failed.as_slice(), [(Step::Launch { .. }, Error::NotPermitted)]), "{:?}", s.failed);
    assert!(k.calls.contains(&Call::Destroy(budget)), "{:?}", k.calls);
    assert_eq!(s.holding(), before, "nothing of the lease is held");
    assert_eq!(s.launching(), 0);
}

/// A second `start_agent` while a launch waits: its batch runs to its launch and queues behind
/// the first, both calls parked, and each is answered by its own report, in order.
#[test]
fn a_second_start_agent_during_a_launch_queues_and_both_are_answered_in_order() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    k.launch_pends = true;
    let first = call_parked(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 }), 3);
    let second = call_parked(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 }), 4);
    assert_eq!(s.launching(), 2);
    let outputs = s.launched(&mut k, 1).unwrap();
    let Ok(Reply::StartAgent(a)) = late(&s, &first, &outputs, 3) else { panic!("{outputs:?}") };
    assert_eq!(a.name, "agent-1");
    assert!(!outputs.iter().any(|o| matches!(o, Output::Reply { to: 4, .. })), "the second still waits");
    assert!(s.awaits(4));
    let outputs = s.launched(&mut k, 2).unwrap();
    let Ok(Reply::StartAgent(b)) = late(&s, &second, &outputs, 4) else { panic!("{outputs:?}") };
    assert_eq!(b.name, "agent-2");
    assert_eq!(s.launching(), 0);
}

/// A login's launch runs on the thread too, the login parked; `sshd`'s end meanwhile, which
/// detaches every attached context, is held while a session starts (the core takes a starting
/// session's detach only after its `Done`) and runs after the report: the login is answered,
/// then the new context is detached.
#[test]
fn sshd_s_end_waits_for_a_logins_launch_and_detaches_the_context_after() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    k.launch_pends = true;
    let parked = call_parked(&mut s, &mut k, SSHD, login("bob", "", &BOB_KEY), 3);
    k.calls.clear();
    s.event(&mut k, EventKind::SshdGone, 0).unwrap();
    assert!(k.calls.is_empty(), "held: {:?}", k.calls);
    let job = k.jobs;
    let outputs = s.launched(&mut k, job).unwrap();
    let Ok(Reply::Login(r)) = late(&s, &parked, &outputs, 3) else { panic!("{outputs:?}") };
    assert!(r.session >= MINTED);
    let launched = k.calls.iter().position(|c| matches!(c, Call::Launched(_))).unwrap();
    let detached = k.calls.iter().position(|c| matches!(c, Call::Detach { .. })).expect("detached after");
    assert!(launched < detached, "{:?}", k.calls);
    assert!(s.exited(pid_of(&s, Kind::Session)).is_some(), "the VM runs on, detached");
}

/// The deadline witness's pair, the clock read and the destroy by a kept handle, runs under
/// the install lock, with nothing between them and no handle installed under it (the fake
/// kernel refuses one): a live lease's end destroys inside the bracket; a lease the deadline
/// took makes no destroy inside it.
#[test]
fn the_destroy_pair_runs_under_the_install_lock_and_nothing_installs_under_it() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let (lease, budget, _) = lease_for(&mut s, &mut k, u, 60_000_000);
    k.calls.clear();
    let Ok(Reply::EndLease(_)) = call(&mut s, &mut k, u, Message::EndLease(EndLease { lease })) else {
        panic!()
    };
    let locked = k.calls.iter().position(|c| *c == Call::Locked).unwrap();
    assert_eq!(
        &k.calls[locked..locked + 3],
        &[Call::Locked, Call::Destroy(budget), Call::Unlocked],
        "{:?}",
        k.calls
    );
    let (_, budget, pid) = lease_for(&mut s, &mut k, u, 2_000_000);
    k.clock += 2_000_000;
    k.deadline_takes(budget);
    k.calls.clear();
    let object = s.exited(pid).unwrap();
    s.event(&mut k, EventKind::Exited { object }, 0).unwrap();
    let locked = k.calls.iter().position(|c| *c == Call::Locked).unwrap();
    assert_eq!(
        &k.calls[locked..locked + 2],
        &[Call::Locked, Call::Unlocked],
        "no destroy by the revoked handle: {:?}",
        k.calls
    );
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
            Call::Hand(kept),
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
    assert_eq!(k.calls.len(), 5, "nothing else but the server's steps forgotten: {:?}", k.calls);
    assert_eq!(k.calls[1], Call::Forget("walfsd:data".into()));
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
    assert!(
        matches!(k.calls[1..], [Call::Release(a), Call::Hand(b)] if a == b),
        "the dead connection's record goes, its step the thread's: {:?}",
        k.calls
    );
    assert_eq!(k.calls.len(), 3);
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
    assert_eq!(k.calls, [Call::Detach { relay, note: String::new() }, Call::Release(kept), Call::Hand(kept)]);
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
    assert_eq!(k.calls, [Call::Detach { relay: 8, note: String::new() }, Call::Release(9), Call::Hand(9)]);
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

/// The steps the cleanup thread ran, in order: no event of the loop's makes one.
fn cleaned(k: &Recorder) -> Vec<(usize, bool)> {
    k.calls
        .iter()
        .filter_map(|c| if let Call::Clean { handle, parks } = c { Some((*handle, *parks)) } else { None })
        .collect()
}

/// The connections the loop handed to the thread, in order.
fn handed(k: &Recorder) -> Vec<usize> {
    k.calls.iter().filter_map(|c| if let Call::Hand(h) = c { Some(*h) } else { None }).collect()
}

/// The servers the loop woke, in order.
fn woken(k: &Recorder) -> Vec<&str> {
    k.calls.iter().filter_map(|c| if let Call::Wake(s) = c { Some(s.as_str()) } else { None }).collect()
}

/// The mints of connections at the system volume (slot 5) the loop made.
fn mints_at_system(k: &Recorder) -> usize {
    k.calls.iter().filter(|c| matches!(c, Call::Connect { slot: 5, .. })).count()
}

/// A session logged in and ended by its own `end_session`, with the kernel's record cleared
/// before the end: what the end batch left in the cleanup queue.
fn ended_session(s: &mut Steward<usize, usize>, k: &mut Recorder, principal: &str, key: &[u8]) {
    let Ok(Reply::Login(r)) = call(s, k, SSHD, login(principal, "", key)) else { panic!() };
    let badge = badge_of(s, r.session);
    k.calls.clear();
    assert!(matches!(call(s, k, badge, Message::EndSession(EndSession {})), Ok(Reply::EndSession(_))));
}

/// The end batch is answered once its scope is destroyed, the cutoff; the servers are told by
/// the cleanup thread (servers/steward.md, "Failure and restart"): no `clean` runs inside the
/// event, every connection the end released is handed over once, in the order it was released,
/// and the thread's turn tells each that has a server, once; a connection nobody is told is done
/// at the hand-over.
#[test]
fn a_session_s_end_is_answered_before_any_cleanup_call() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    let released = released(&k);
    assert!(released.len() >= 6, "{:?}", k.calls);
    assert!(cleaned(&k).is_empty(), "no server is told inside the event: {:?}", k.calls);
    assert_eq!(handed(&k), released, "each connection handed over once, in order: {:?}", k.calls);
    let told: Vec<usize> = released.iter().copied().filter(|h| k.at.contains_key(h)).collect();
    assert!(told.len() >= 5 && told.len() < released.len(), "{told:?} of {released:?}");
    assert_eq!(k.pending(), told.len(), "the steps are the connections with a server");
    k.calls.clear();
    assert_eq!(k.cleanup.due(), Some(k.now()), "due at once");
    k.thread();
    assert_eq!(cleaned(&k), told.iter().map(|&h| (h, false)).collect::<Vec<_>>());
    assert_eq!((k.pending(), k.cleanup.due()), (0, None), "nothing is left to retry");
    k.calls.clear();
    assert!(k.thread().is_empty());
    assert!(k.calls.is_empty(), "an empty queue makes no call");
}

/// The loop's turn makes no call to a shared server, whatever the thread's steps meet
/// (servers/steward.md, "Failure and restart"): with the system volume never answering, bob's
/// end hands its steps over and calls nobody; the thread's turn makes the calls and gets no
/// answer; alice's login and end, events of the loop's, make no cleanup call either and wait for
/// nothing, since nothing is parked yet; every cleanup call recorded is the thread's.
#[test]
fn the_loops_turn_makes_no_call_to_a_server_that_never_answers() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    k.busy = vec!["erofsd:system".into()];
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    assert!(cleaned(&k).is_empty() && !handed(&k).is_empty(), "{:?}", k.calls);
    k.calls.clear();
    k.thread();
    let system = released(&k).len();
    assert!(!cleaned(&k).is_empty(), "the thread's turn made the calls: {:?}", k.calls);
    assert_eq!(k.pending(), 1, "the one the busy server did not take is kept: {:?}", k.calls);
    k.calls.clear();
    ended_session(&mut s, &mut k, "alice", &ALICE_KEY);
    assert!(cleaned(&k).is_empty(), "no cleanup call inside the loop's events: {:?}", k.calls);
    assert!(woken(&k).is_empty(), "nothing parked: nothing woken: {:?}", k.calls);
    assert_eq!(system, 0, "the step's record is the thread's business alone");
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
    assert!(cleaned(&k).is_empty() && k.pending() > 0, "the servers are still to be told: {:?}", k.calls);
    let c = open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap();
    let screen = screens(&k, c).pop().expect("the channel's first screen");
    assert!(
        !screen.contains(&format!("request {:016x} ", r.request)),
        "withdrawn with her session: {screen}"
    );
    assert!(!screen.contains("\"mine\""), "{screen}");
    assert!(screen.starts_with("no approval is waiting for alice"), "{screen}");
}

/// One turn of the thread at `steps` due steps whose servers all time out: each is tried once,
/// and the thread moves on; the queue's next due after it.
fn round(k: &mut Recorder, steps: usize) -> Option<u64> {
    let before = cleaned(k).len();
    k.thread();
    assert_eq!(cleaned(k).len(), before + steps, "every due step tried once: {:?}", k.calls);
    k.cleanup.due()
}

/// A step whose server does not take the call within the client's call bound (`Timeout`) is
/// kept and retried no sooner than `RETRY_US` later, up to `TRIES` attempts, the last told it
/// parks; then the step is parked: still held, due at no time. A turn before the retry is due
/// makes no call.
#[test]
fn a_cleanup_step_that_times_out_is_retried_then_parked() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    let pending = k.pending();
    assert!(pending > 0);
    k.clean = Some(Error::Timeout);
    k.calls.clear();
    let now = k.now();
    let due = round(&mut k, pending);
    assert_eq!(due, Some(now + RETRY_US), "the retry is due RETRY_US after the attempt");
    assert!(cleaned(&k).iter().all(|&(_, last)| !last));
    assert_eq!(k.pending(), pending, "a timed-out step is kept");
    k.calls.clear();
    k.clock += RETRY_US - 1;
    assert!(k.thread().is_empty());
    assert!(k.calls.is_empty(), "not due yet: no call");
    for attempt in 2..=TRIES {
        k.clock += RETRY_US;
        k.calls.clear();
        let next = round(&mut k, pending);
        let last = attempt == TRIES;
        assert!(cleaned(&k).iter().all(|&(_, l)| l == last), "attempt {attempt}: {:?}", k.calls);
        if last {
            assert_eq!(
                (next, k.pending(), k.parked()),
                (None, pending, pending),
                "parked after TRIES attempts"
            );
        } else {
            assert!(next.is_some() && k.pending() == pending, "attempt {attempt} is kept");
        }
    }
}

/// A step the server did not take in time holds nobody: the thread moves on to the steps behind
/// it in the same turn, since no caller waits on it now, and the timed-out one is due a second
/// later in its place, the only one left.
#[test]
fn a_timed_out_step_does_not_hold_the_steps_behind_it() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    let pending = k.pending();
    assert!(pending >= 2);
    let first = *released(&k).iter().find(|h| k.at.contains_key(h)).unwrap();
    k.busy = vec![k.at[&first].clone()];
    k.calls.clear();
    let now = k.now();
    k.thread();
    assert_eq!(cleaned(&k).len(), pending, "every step tried in the one turn: {:?}", k.calls);
    assert_eq!(cleaned(&k)[0], (first, false), "the timed-out one first, in its order");
    assert_eq!((k.pending(), k.cleanup.due()), (1, Some(now + RETRY_US)), "only its retry is left");
    k.busy.clear();
    k.clock += RETRY_US;
    k.calls.clear();
    assert_eq!(k.thread(), vec![(first, Fate::Done)]);
    assert_eq!(k.pending(), 0);
}

/// Any outcome but `Timeout` ends a step at once: an `ended` to a channel `sshd` already closed
/// fails `Dead` and is never retried, and a `disconnect` the server refuses is done too; the
/// loop is told the fate, and says nothing of it.
#[test]
fn a_cleanup_step_the_server_refuses_or_cannot_take_is_done_at_once() {
    for error in [Error::Dead, Error::Refused] {
        let mut k = Recorder::default();
        let mut s = started(&mut k);
        ended_session(&mut s, &mut k, "bob", &BOB_KEY);
        let pending = k.pending();
        k.clean = Some(error);
        k.calls.clear();
        let fates = k.thread();
        assert_eq!(cleaned(&k).len(), pending);
        assert!(fates.iter().all(|(_, f)| *f == Fate::Failed(error)), "{fates:?}");
        assert_eq!((k.pending(), k.cleanup.due()), (0, None), "{error:?} ends the step");
    }
}

/// The queue's bound (servers/steward.md, "Failure and restart"): a connection enters once, when
/// its owner's end releases it, so the queue never holds more than the connections the ended
/// objects held, at most `SLOTS + 4` per session (its badge, a connection per slot, its relay's
/// control connection, the attached channel console and its notices send right); a turn of the
/// thread empties it, and an object's second end queues nothing.
#[test]
fn the_cleanup_queue_holds_each_released_connection_once() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let mut all = Vec::new();
    for (principal, key) in [("bob", &BOB_KEY), ("alice", &ALICE_KEY)] {
        ended_session(&mut s, &mut k, principal, key);
        let released = released(&k);
        assert!(released.len() <= SLOTS as usize + 4, "{principal}: {released:?}");
        assert_eq!(handed(&k), released, "{principal}: each handed over once");
        all.extend(released);
    }
    let mut unique = all.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), all.len(), "each connection once: {all:?}");
    let told = all.iter().filter(|h| k.at.contains_key(h)).count();
    assert_eq!(k.pending(), told);
    k.calls.clear();
    k.thread();
    assert_eq!(cleaned(&k).len(), told);
    assert_eq!(k.pending(), 0);
}

/// A step whose call the server never answers holds nothing but its own record: the thread has
/// one call out and takes no other step meanwhile, the loop's events go on and hand their steps
/// over, and the server's restart forgets the rest of its steps while the one out stays out
/// (its call returns within its bound, whatever the server did).
#[test]
fn a_step_that_never_completes_holds_only_itself() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    let now = k.now();
    let (out, tries) = k.cleanup.take(now).expect("the first step");
    assert_eq!((out, tries), (*released(&k).iter().find(|h| k.at.contains_key(h)).unwrap(), 1));
    let server = k.at[&out].clone();
    assert!(k.cleanup.take(now).is_none(), "one call at a time: nothing else is taken while it is out");
    let before = k.pending();
    ended_session(&mut s, &mut k, "alice", &ALICE_KEY);
    assert!(k.pending() > before, "the loop went on and handed alice's steps over");
    assert!(cleaned(&k).is_empty(), "and made no call");
    let queued = k.pending();
    let m = Message::Reconnected(Reconnected { server: &server });
    assert!(matches!(call(&mut s, &mut k, INIT, m), Ok(Reply::Reconnected(_))));
    let now = k.now();
    assert!(k.pending() < queued, "the server's queued steps are forgotten: {:?}", k.calls);
    assert!(k.cleanup.take(now).is_none(), "the step out stays out, and nothing else is taken while it is");
    assert_eq!(
        k.cleanup.settle(Err(Error::Dead), now),
        Some((out, Fate::Failed(Error::Dead))),
        "its late answer settles it"
    );
}

/// A step that completes after the loop moved on is settled then: its late answer ends it, the
/// thread moves on to what the loop handed over meanwhile, and nothing is lost or run twice.
#[test]
fn a_step_that_completes_after_the_loop_moved_on_is_settled_then() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    let now = k.now();
    let (out, _) = k.cleanup.take(now).expect("the first step");
    ended_session(&mut s, &mut k, "alice", &ALICE_KEY);
    let pending = k.pending();
    k.calls.clear();
    let now = k.now();
    assert_eq!(k.cleanup.settle(Ok(()), now), Some((out, Fate::Done)));
    assert_eq!(k.pending(), pending - 1);
    k.thread();
    assert_eq!(
        cleaned(&k).len(),
        pending - 1,
        "the rest ran once each, the settled one not again: {:?}",
        k.calls
    );
    assert_eq!(k.pending(), 0);
}

/// A console that records what the steward's lines cost it (servers/steward.md, "Failure and
/// restart"): each open of the fid and each write, and what it answers.
#[derive(Default)]
struct Paper {
    calls: Vec<String>,
    /// What every open answers, and every write: `Ok` unless a test says.
    open: Option<Error>,
    write: Option<Error>,
    /// The most a write takes: a line longer goes in several.
    iounit: Option<usize>,
}

impl Sink for Paper {
    fn open(&mut self) -> Result<(), Error> {
        self.calls.push("open".into());
        self.open.map_or(Ok(()), Err)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        let n = self.iounit.map_or(bytes.len(), |u| u.min(bytes.len()));
        self.calls.push(format!("write {}", String::from_utf8_lossy(&bytes[..n])));
        self.write.map_or(Ok(n), Err)
    }
}

fn queued(lines: &[&str]) -> Queue {
    let mut q = Queue::default();
    for l in lines {
        q.push((*l).into());
    }
    q
}

/// A line is one write on the fid the first turn opened and every later turn keeps: no attach,
/// open or close per line; a line longer than the console's `iounit` goes in several writes,
/// each of the rest.
#[test]
fn a_console_line_is_one_write_on_the_kept_fid() {
    let mut p = Paper::default();
    let mut q = queued(&["a\n", "b\n"]);
    assert_eq!(q.run(0, &mut p), None, "nothing is left");
    assert_eq!(p.calls, ["open", "write a\n", "write b\n"]);
    q.push("c\n".into());
    p.calls.clear();
    assert_eq!(q.run(1, &mut p), None);
    assert_eq!(p.calls, ["write c\n"], "the fid is kept: one call per line");
    p.iounit = Some(3);
    p.calls.clear();
    q.push("long line\n".into());
    assert_eq!(q.run(2, &mut p), None);
    assert_eq!(p.calls, ["write lon", "write g l", "write ine", "write \n"]);
}

/// A write the console did not take within the bound (`Timeout`) ends the turn: one call, the
/// line and every line behind it kept in order, the next attempt due `RETRY_US` later and no
/// call before it; the console answering, the next turn writes them all in the order they were
/// said. After `TRIES` attempts the line is dropped, counted, and the lines behind it go, the
/// count said first.
#[test]
fn a_console_line_the_console_did_not_take_costs_one_bound_and_keeps_the_order() {
    let mut p = Paper::default();
    let mut q = queued(&["first\n", "second\n", "third\n"]);
    p.write = Some(Error::Timeout);
    let now = 1_000;
    assert_eq!(q.run(now, &mut p), Some(now + RETRY_US), "the retry is due RETRY_US later");
    assert_eq!(p.calls, ["open", "write first\n"], "one wait per turn");
    assert_eq!(q.len(), 3, "the line and the lines behind it are kept");
    p.calls.clear();
    assert_eq!(q.run(now + RETRY_US - 1, &mut p), Some(now + RETRY_US));
    assert!(p.calls.is_empty(), "not due yet: no call");
    q.push("fourth\n".into());
    p.write = None;
    assert_eq!(q.run(now + RETRY_US, &mut p), None);
    assert_eq!(p.calls, ["write first\n", "write second\n", "write third\n", "write fourth\n"], "in order");
    assert_eq!(q.len(), 0);
    // Dropped after TRIES attempts, said by its count before the next line.
    let mut p = Paper::default();
    let mut q = queued(&["lost\n", "kept\n"]);
    p.write = Some(Error::Timeout);
    let mut now = 0;
    for attempt in 1..TRIES {
        assert_eq!(q.run(now, &mut p), Some(now + RETRY_US), "attempt {attempt}");
        now += RETRY_US;
    }
    assert_eq!(q.run(now, &mut p), Some(now), "the last attempt drops the line and the next is due now");
    assert_eq!((q.len(), q.dropped()), (1, 1));
    p.write = None;
    p.calls.clear();
    assert_eq!(q.run(now, &mut p), None);
    assert_eq!(
        p.calls,
        ["write steward: 1 console lines dropped: the console did not take them\n", "write kept\n"]
    );
    assert_eq!(q.dropped(), 0);
}

/// A console that refuses a write or is gone (a restarted `consoled` knows none of its clients'
/// fids: the kept fid is dead there) drops every line queued, counted, and the next turn opens
/// the fid again, no sooner than `RETRY_US` later, and says the count first: a line goes only
/// with a line saying so. A console that refuses the open too keeps costing one call a turn and
/// nothing is kept for it.
#[test]
fn the_console_fid_is_re_opened_after_a_failed_write_and_the_lines_dropped_are_said() {
    for error in [Error::Dead, Error::Refused] {
        let mut p = Paper::default();
        let mut q = queued(&["one\n", "two\n"]);
        assert_eq!(q.run(0, &mut p), None);
        p.write = Some(error);
        q.push("three\n".into());
        q.push("four\n".into());
        p.calls.clear();
        assert_eq!(q.run(10, &mut p), Some(10 + RETRY_US), "{error:?}: the re-open waits RETRY_US");
        assert_eq!(p.calls, ["write three\n"], "{error:?}: one call, then the queue goes");
        assert_eq!((q.len(), q.dropped()), (0, 2), "{error:?}: both lines dropped and counted");
        p.open = Some(error);
        p.write = None;
        p.calls.clear();
        q.push("five\n".into());
        assert_eq!(q.run(10 + RETRY_US, &mut p), Some(10 + 2 * RETRY_US), "{error:?}: the open refused");
        assert_eq!(p.calls, ["open"], "{error:?}: one call a turn while the console is gone");
        assert_eq!((q.len(), q.dropped()), (0, 3));
        p.open = None;
        p.calls.clear();
        q.push("six\n".into());
        assert_eq!(q.run(10 + 2 * RETRY_US, &mut p), None, "{error:?}: the console answers again");
        assert_eq!(
            p.calls,
            [
                "open",
                "write steward: 3 console lines dropped: the console did not take them\n",
                "write six\n"
            ],
            "{error:?}: the fid opened again, the count first"
        );
        assert_eq!(q.dropped(), 0);
    }
}

/// The last turn, at the steward's exit, writes every line whatever is due, up to the first
/// the console did not take.
#[test]
fn the_exit_s_last_turn_writes_the_lines_left_within_one_bound() {
    let mut p = Paper::default();
    let mut q = queued(&["a\n", "b\n"]);
    p.write = Some(Error::Timeout);
    assert_eq!(q.run(0, &mut p), Some(RETRY_US));
    p.write = None;
    p.calls.clear();
    q.flush(&mut p);
    assert_eq!(p.calls, ["write a\n", "write b\n"], "not due, written anyway");
    assert!(q.is_empty());
    p.write = Some(Error::Timeout);
    q.push("c\n".into());
    q.push("d\n".into());
    p.calls.clear();
    q.flush(&mut p);
    assert_eq!(p.calls, ["write c\n"], "one bound, then the steward is gone");
}

/// Bob's session ended while `server` takes no cleanup step: the step at it is parked after
/// `TRIES` attempts and every other step is done; the handle parked, the calls cleared.
fn parked_at(s: &mut Steward<usize, usize>, k: &mut Recorder, server: &str) -> usize {
    k.busy = vec![server.into()];
    ended_session(s, k, "bob", &BOB_KEY);
    let handle = *released(k)
        .iter()
        .find(|h| k.at.get(h).map(String::as_str) == Some(server))
        .expect("the session held a connection at the server");
    let mut fates = Vec::new();
    while k.parked() == 0 {
        fates.extend(k.thread());
        k.clock += RETRY_US;
    }
    assert!(k.thread().is_empty());
    assert_eq!((k.pending(), k.parked()), (1, 1), "{:?}", k.calls);
    assert_eq!(
        cleaned(k).iter().filter(|(_, parks)| *parks).count(),
        1,
        "the parking attempt, once: {:?}",
        k.calls
    );
    assert_eq!(
        fates.iter().filter(|(h, _)| *h == handle).collect::<Vec<_>>(),
        [&(handle, Fate::Parked(TRIES))]
    );
    k.calls.clear();
    handle
}

/// The position of the first call `c` matches in `k`.
fn position(k: &Recorder, c: impl Fn(&Call) -> bool) -> usize {
    k.calls.iter().position(c).expect("the call")
}

/// A parked step runs again when its server answers a call of the steward's (servers/steward.md,
/// "Failure and restart"). Bob's `disconnect` at `erofsd:system` is parked. Alice's login mints
/// there: the loop wakes the parked step before the mint and again on the mint's answer, making
/// no cleanup call of its own; the thread runs the step (its server still takes none: one bound,
/// the thread's) and parks it again, past the attempt that says the line, so with no second
/// line. Then the server takes steps again: alice's end's step there is done, and bob's, woken
/// by it, runs after it; nothing is left.
#[test]
fn a_parked_step_runs_at_the_servers_next_answered_call() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let bobs = parked_at(&mut s, &mut k, "erofsd:system");
    let Ok(Reply::Login(a)) = call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)) else { panic!() };
    assert!(cleaned(&k).is_empty(), "the loop made no cleanup call: {:?}", k.calls);
    assert_eq!(
        woken(&k),
        ["erofsd:system"],
        "woken before the mint; its answer finds nothing parked: {:?}",
        k.calls
    );
    assert!(
        position(&k, |c| matches!(c, Call::Wake(_)))
            < position(&k, |c| matches!(c, Call::Connect { slot: 5, .. }))
    );
    k.calls.clear();
    assert_eq!(k.thread(), vec![(bobs, Fate::Parked(TRIES + 1))], "run once by the thread, parked again");
    assert_eq!(cleaned(&k), vec![(bobs, false)], "past the attempt that says the line");
    assert_eq!((k.pending(), k.parked()), (1, 1));
    k.calls.clear();
    for _ in 0..3 {
        k.clock += RETRY_US;
        assert!(k.thread().is_empty());
    }
    assert!(cleaned(&k).is_empty(), "parked: no attempt on a timer: {:?}", k.calls);
    k.busy.clear();
    let badge = badge_of(&s, a.session);
    k.calls.clear();
    assert!(matches!(
        call(&mut s, &mut k, badge, Message::EndSession(EndSession {})),
        Ok(Reply::EndSession(_))
    ));
    let alices =
        *released(&k).iter().find(|h| k.at.get(h).map(String::as_str) == Some("erofsd:system")).unwrap();
    k.calls.clear();
    k.thread();
    let done = cleaned(&k);
    let answered = done.iter().position(|&(h, _)| h == alices).expect("alice's step there");
    let retried = done.iter().position(|&(h, _)| h == bobs).expect("bob's parked step");
    assert!(answered < retried, "the server's answer first, then the parked step: {done:?}");
    assert_eq!((k.pending(), k.parked()), (0, 0));
    assert_eq!(k.records("erofsd:system"), 0, "the server was told of every connection");
}

/// A mint at a server with parked steps wakes them first, since it needs the record a step frees,
/// and waits for nothing: refused all the same, it fails its step and the caller is told, as a
/// mint the server does not take in time is (servers/steward.md, "Failure and restart"). Bob's
/// step at `erofsd:system` is parked and his connection's record fills the server; alice's
/// login there wakes it, the mint is refused once and the login fails; the thread's turn runs
/// the step (the server still takes none: parked again); when the server takes steps again the
/// next login wakes it, the thread frees the record, and the login after it succeeds.
#[test]
fn a_mint_at_a_server_with_parked_steps_wakes_them_and_the_login_after_succeeds() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let bobs = parked_at(&mut s, &mut k, "erofsd:system");
    let full = k.records("erofsd:system");
    assert!(full >= 1, "bob's record leaked there");
    k.cap = Some(("erofsd:system".into(), full));
    let e = call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)).unwrap_err();
    assert_eq!(e, ErrorCode::Failed, "the mint is refused at the leaked record");
    assert!(cleaned(&k).is_empty(), "the loop made no cleanup call and waited for none: {:?}", k.calls);
    assert!(
        position(&k, |c| matches!(c, Call::Wake(_)))
            < position(&k, |c| matches!(c, Call::Connect { slot: 5, .. }))
    );
    assert_eq!(mints_at_system(&k), 1, "refused once, not retried: {:?}", k.calls);
    // The refusal is an answer: the failed batch's own connections are handed over, and the
    // thread's next turn tells them and parks bob's again.
    k.calls.clear();
    let fates = k.thread();
    assert!(fates.contains(&(bobs, Fate::Parked(TRIES + 1))), "{fates:?}");
    assert!(k.thread().is_empty());
    assert_eq!((k.pending(), k.parked()), (1, 1), "still parked: the server took no step");
    k.busy.clear();
    k.calls.clear();
    let e = call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)).unwrap_err();
    assert_eq!(e, ErrorCode::Failed, "the record is still there until the thread runs: {:?}", k.calls);
    let fates = k.thread();
    assert!(fates.contains(&(bobs, Fate::Done)), "woken by the mint, run by the thread: {fates:?}");
    assert!(fates.iter().all(|(_, f)| *f == Fate::Done), "and the failed batch's steps with it: {fates:?}");
    k.calls.clear();
    let Ok(Reply::Login(_)) = call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)) else {
        panic!("the parked step freed the record: {:?}", k.calls)
    };
    assert_eq!(mints_at_system(&k), 1);
    assert_eq!((k.pending(), k.parked()), (0, 0));
    assert_eq!(k.records("erofsd:system"), full, "bob's record went, alice's took its place");
}

/// `reconnected` for a server forgets the steps parked there with its records: what they would
/// have freed died with the old instance. Bob's steps at `walfsd:data` and `erofsd:system` are
/// parked; `init` reports `walfsd:data` restarted; the step there is gone and never run, the one
/// at `erofsd:system` still waits and runs when that server answers.
#[test]
fn reconnected_forgets_a_servers_parked_steps() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    k.busy = vec!["walfsd:data".into(), "erofsd:system".into()];
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    let at = |k: &Recorder, server: &str| -> usize {
        *released(k).iter().find(|h| k.at.get(h).map(String::as_str) == Some(server)).unwrap()
    };
    let (home, system) = (at(&k, "walfsd:data"), at(&k, "erofsd:system"));
    while k.parked() < 2 {
        k.thread();
        k.clock += RETRY_US;
    }
    assert!(k.thread().is_empty());
    assert_eq!((k.pending(), k.parked()), (2, 2), "{:?}", k.calls);
    k.calls.clear();
    let m = Message::Reconnected(Reconnected { server: "walfsd:data" });
    let Ok(Reply::Reconnected(_)) = call(&mut s, &mut k, INIT, m) else { panic!() };
    assert_eq!((k.pending(), k.parked()), (1, 1), "the home's step is forgotten: {:?}", k.calls);
    assert!(k.calls.contains(&Call::Forget("walfsd:data".into())), "{:?}", k.calls);
    // The servers take steps again: alice's login mints at the system volume, so the loop wakes
    // the step parked there and the thread runs it; nothing ever runs for the forgotten one.
    k.busy.clear();
    k.calls.clear();
    let Ok(Reply::Login(_)) = call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)) else { panic!() };
    assert_eq!(k.thread(), vec![(system, Fate::Done)], "the system volume's parked step ran: {:?}", k.calls);
    assert_eq!(cleaned(&k), vec![(system, false)]);
    assert!(!cleaned(&k).iter().any(|&(h, _)| h == home), "the forgotten step never runs: {:?}", k.calls);
    assert!(k.at.contains_key(&home), "its record died with the old instance, told nothing");
    assert_eq!((k.pending(), k.parked()), (0, 0));
}

/// A restarted server's steps still due, not yet parked, die with its records the same way:
/// bob's step at `walfsd:data` has not been tried when `init` reports the restart, and it is
/// never run.
#[test]
fn reconnected_forgets_a_servers_due_steps_too() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    ended_session(&mut s, &mut k, "bob", &BOB_KEY);
    let home = *released(&k).iter().find(|h| k.at.get(h).map(String::as_str) == Some("walfsd:data")).unwrap();
    let pending = k.pending();
    let m = Message::Reconnected(Reconnected { server: "walfsd:data" });
    let Ok(Reply::Reconnected(_)) = call(&mut s, &mut k, INIT, m) else { panic!() };
    assert_eq!(k.pending(), pending - 1, "{:?}", k.calls);
    k.calls.clear();
    k.thread();
    assert!(!cleaned(&k).iter().any(|&(h, _)| h == home), "never run: {:?}", k.calls);
    assert!(k.at.contains_key(&home));
    assert_eq!(k.pending(), 0);
}

/// A server that never answers again costs nobody anything once its steps are parked: no
/// attempt on a timer, no bound per turn, no wake; the step is held, once, until the server
/// answers or restarts.
#[test]
fn a_never_answering_server_costs_no_bound_once_its_steps_are_parked() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    parked_at(&mut s, &mut k, "erofsd:system");
    for _ in 0..10 {
        k.clock += RETRY_US;
        assert!(k.thread().is_empty(), "nothing is due at any time");
        assert_eq!(k.cleanup.due(), None);
    }
    assert!(k.calls.is_empty(), "no call: {:?}", k.calls);
    assert_eq!((k.pending(), k.parked()), (1, 1), "held once");
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
    assert!(
        matches!(k.calls.as_slice(), [Call::Detach { .. }, Call::Release(_), Call::Hand(_)]),
        "{:?}",
        k.calls
    );
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

/// The budget of the lease started last: the one made with a deadline (its scope follows, for
/// ever).
fn leased(k: &Recorder) -> usize { k.deadlines.iter().rev().find(|d| d.1 != FOREVER).map(|d| d.0).unwrap() }

/// A lease started for `us` microseconds by `u`: its id, its budget's handle and its pid.
fn lease_for(s: &mut Steward<usize, usize>, k: &mut Recorder, u: u64, us: u64) -> (u64, usize, u64) {
    let Ok(Reply::StartAgent(a)) = call(s, k, u, Message::StartAgent(StartAgent { lease: us })) else {
        panic!()
    };
    let budget = leased(k);
    let pid = (1..200).find(|p| s.exited(*p).is_some_and(|o| o.id == a.lease)).expect("the lease's process");
    (a.lease, budget, pid)
}

/// A lease the kernel's deadline took: its process's exit notice arrives with the budget gone,
/// and the core's `DestroyBudget` on it is the end asked for, done already: the steward makes no
/// destroy by the handle the deadline revoked, and no step fails; the sponsor is told all the
/// same. A budget whose handle answers `BadHandle` before its deadline is a handle the steward
/// lost, and that step fails.
#[test]
fn a_leases_deadline_ends_it_without_a_failed_step() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let (lease, budget, pid) = lease_for(&mut s, &mut k, u, 2_000_000);
    k.clock += 2_000_000;
    k.deadline_takes(budget);
    k.calls.clear();
    let object = s.exited(pid).unwrap();
    let outputs = s.event(&mut k, EventKind::Exited { object }, 0).unwrap();
    assert!(s.failed.is_empty(), "{:?}", s.failed);
    assert!(!k.calls.iter().any(|c| matches!(c, Call::Destroy(_))), "{:?}", k.calls);
    assert!(
        k.calls.iter().any(
            |c| matches!(c, Call::Notify { lease: l, name, .. } if *l == lease && name == "alice/agent-1")
        )
    );
    assert!(
        !outputs.iter().any(|o| matches!(o, Output::Notice { .. })),
        "the notice went, it is not left over"
    );
    assert!(k.calls.iter().any(|c| matches!(c, Call::Release(_))), "its connections are given back");
    let (lease, budget, _) = lease_for(&mut s, &mut k, u, 2_000_000);
    k.lost.push(budget);
    let Ok(Reply::EndLease(_)) = call(&mut s, &mut k, u, Message::EndLease(EndLease { lease })) else {
        panic!()
    };
    assert_eq!(s.failed.len(), 1);
}

/// Exits that arrived together, the leases a shared deadline took: every sponsor notice goes
/// before any exit's bookkeeping (no connection given back, no step handed over, no destroy
/// ahead of the last notice), each lease's once and in the exits' order; every exit's record
/// comes out; and one exit through the burst path makes the calls one through the event path
/// makes. The core sees the same rows either way: an exit moves its lease to `Ending` by itself,
/// and its `Done` follows its own batch.
#[test]
fn a_bursts_notices_all_go_before_any_exits_bookkeeping() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let leases: Vec<(u64, usize, u64)> = (0..3).map(|_| lease_for(&mut s, &mut k, u, 2_000_000)).collect();
    k.clock += 2_000_000;
    for (_, budget, _) in &leases {
        k.deadline_takes(*budget);
    }
    k.calls.clear();
    let objects: Vec<Object> = leases.iter().map(|(_, _, pid)| s.exited(*pid).unwrap()).collect();
    let outputs = s.exits(&mut k, objects).unwrap();
    assert!(s.failed.is_empty(), "{:?}", s.failed);
    let names: Vec<&str> = k
        .calls
        .iter()
        .filter_map(|c| match c {
            Call::Notify { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        ["alice/agent-1", "alice/agent-2", "alice/agent-3"],
        "each lease's notice once, in order"
    );
    let last_notice = k.calls.iter().rposition(|c| matches!(c, Call::Notify { .. })).unwrap();
    let first_work = k
        .calls
        .iter()
        .position(|c| matches!(c, Call::Release(_) | Call::Hand(_) | Call::Destroy(_)))
        .expect("the exits give their connections back");
    assert!(last_notice < first_work, "a notice waited behind another exit's work: {:?}", k.calls);
    let ended = outputs
        .iter()
        .filter(|o| matches!(o, Output::Audit(a) if matches!(a.record(), Record::LeaseEnded { .. })))
        .count();
    assert_eq!(ended, 3, "every exit's record: {outputs:?}");
    for (lease, _, pid) in &leases {
        assert!(s.exited(*pid).is_none() && s.lease_name(*lease).is_none(), "the lease is gone");
    }
    // One exit each way: the same calls in the same order.
    let shape = |calls: &[Call]| calls.iter().map(std::mem::discriminant).collect::<Vec<_>>();
    let (_, budget, pid) = lease_for(&mut s, &mut k, u, 2_000_000);
    k.clock += 2_000_000;
    k.deadline_takes(budget);
    k.calls.clear();
    let object = s.exited(pid).unwrap();
    s.exits(&mut k, vec![object]).unwrap();
    let burst = shape(&k.calls);
    let (_, budget, pid) = lease_for(&mut s, &mut k, u, 2_000_000);
    k.clock += 2_000_000;
    k.deadline_takes(budget);
    k.calls.clear();
    let object = s.exited(pid).unwrap();
    s.event(&mut k, EventKind::Exited { object }, 0).unwrap();
    assert_eq!(burst, shape(&k.calls), "one exit is one exit");
    assert!(s.failed.is_empty(), "{:?}", s.failed);
}

/// A lease's deadline passes and the kernel takes its budget, revoking the steward's handle and
/// freeing its index, which a new lease's budget takes before the first's exit notice reaches
/// the steward (several leases expiring close together do this): the first's `DestroyBudget`
/// is not made by that index, and the new budget survives in the kernel. The verdict is the
/// fake kernel's: the new budget is live after the notice, and its own `end_lease` destroys it.
#[test]
fn a_leases_index_recycled_for_a_new_budget_before_its_notice_is_not_destroyed_as_it() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let (first, budget, pid) = lease_for(&mut s, &mut k, u, 2_000_000);
    k.clock += 2_000_000;
    k.deadline_takes(budget);
    let (second, recycled, _) = lease_for(&mut s, &mut k, u, 60_000_000);
    assert_eq!(recycled, budget, "the new lease's budget took the freed index");
    assert!(k.budgets.contains(&recycled));
    k.calls.clear();
    let object = s.exited(pid).unwrap();
    s.event(&mut k, EventKind::Exited { object }, 0).unwrap();
    assert!(s.failed.is_empty(), "{:?}", s.failed);
    assert!(k.budgets.contains(&recycled), "the new budget survives the old lease's end");
    assert!(!k.calls.contains(&Call::Destroy(recycled)), "{:?}", k.calls);
    assert!(k.calls.iter().any(|c| matches!(c, Call::Notify { lease, .. } if *lease == first)));
    k.calls.clear();
    let Ok(Reply::EndLease(_)) = call(&mut s, &mut k, u, Message::EndLease(EndLease { lease: second }))
    else {
        panic!()
    };
    assert!(k.calls.contains(&Call::Destroy(recycled)), "{:?}", k.calls);
    assert!(!k.budgets.contains(&recycled));
    assert!(s.failed.is_empty(), "{:?}", s.failed);
}

/// The freed index taken by something that is no budget (a session re-binding its slot at a
/// restarted server): the old lease's end is no `WrongObject`, no failed step.
#[test]
fn a_leases_index_recycled_for_a_connection_fails_no_step() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let (_, budget, pid) = lease_for(&mut s, &mut k, u, 2_000_000);
    k.clock += 2_000_000;
    k.deadline_takes(budget);
    let m = Message::Reconnected(Reconnected { server: "walfsd:data" });
    let Ok(Reply::Reconnected(_)) = call(&mut s, &mut k, INIT, m) else { panic!() };
    let Ok(Reply::Rebind(_)) = call(&mut s, &mut k, u, Message::Rebind(Rebind { slot: 1 })) else { panic!() };
    assert_eq!(k.at.get(&budget).map(String::as_str), Some("walfsd:data"), "the connection took the index");
    k.calls.clear();
    let object = s.exited(pid).unwrap();
    s.event(&mut k, EventKind::Exited { object }, 0).unwrap();
    assert!(s.failed.is_empty(), "{:?}", s.failed);
    assert!(!k.calls.iter().any(|c| matches!(c, Call::Destroy(_))), "{:?}", k.calls);
    assert_eq!(k.at.get(&budget).map(String::as_str), Some("walfsd:data"), "the connection is untouched");
}

/// An `end_lease` served after the lease's deadline passed, before its exit notice: the kernel
/// has the budget already, so the end batch makes no destroy by the revoked handle, whatever
/// took its index; the late notice then gets its line and no event.
#[test]
fn an_end_lease_after_the_deadline_makes_no_destroy_by_the_revoked_handle() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let (first, budget, pid) = lease_for(&mut s, &mut k, u, 2_000_000);
    k.clock += 2_000_000;
    k.deadline_takes(budget);
    let (_, recycled, _) = lease_for(&mut s, &mut k, u, 60_000_000);
    assert_eq!(recycled, budget);
    k.calls.clear();
    let Ok(Reply::EndLease(_)) = call(&mut s, &mut k, u, Message::EndLease(EndLease { lease: first })) else {
        panic!()
    };
    assert!(s.failed.is_empty(), "{:?}", s.failed);
    assert!(!k.calls.iter().any(|c| matches!(c, Call::Destroy(_))), "{:?}", k.calls);
    assert!(k.budgets.contains(&recycled), "the new budget survives");
    assert!(s.exited(pid).is_none());
    assert!(s.ended(pid).is_some_and(|(o, name)| o.id == first && name == "alice/agent-1"));
}

/// Sixteen leases of one account started back to back, as `lease-code-load` starts them, each
/// a budget, a scope, a badge, three connections and an exit endpoint in the steward's table,
/// the handles it holds for the child closed at the launch; then every lease ends by its own
/// exit, close together, in each order the gate's boots ended them (the two-hart rv64 boot:
/// agent-1 to agent-10, then agent-14, after which agent-11 was reported killed there; the rv32
/// boots: 1 to 13, 16, 14, then 15 reported killed; and 1 to 13, 16, 15, then 14), the machine's table
/// modelled whole (`recycles`: every close frees its index for the next handle made, a destroy
/// revokes the budget's handle and its scope's, the exit endpoint is closed before the exit's
/// event). The verdict is the fake kernel's: every `DestroyBudget` destroys the budget created
/// for the lease whose exit it ends, and every lease still running has its budget live after
/// it; and the pids map names, at each notice, the lease whose exit endpoint reported.
#[test]
fn sixteen_leases_ending_by_exit_each_destroy_their_own_budget_and_no_other() {
    let orders: [[usize; 16]; 3] = [
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 14, 11, 12, 13, 15, 16],
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, 14, 15],
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, 15, 14],
    ];
    for order in orders {
        let mut k = Recorder { recycles: true, ..Recorder::default() };
        let mut s = started(&mut k);
        let u = alice_session(&mut s, &mut k);
        let leases: Vec<(u64, usize, u64)> =
            (0..16).map(|_| lease_for(&mut s, &mut k, u, 1_800_000_000)).collect();
        let budgets: BTreeSet<usize> = leases.iter().map(|l| l.1).collect();
        assert_eq!(budgets.len(), 16, "sixteen distinct budget handles: {leases:?}");
        assert!(budgets.iter().all(|b| k.budgets.contains(b)));
        let pids: BTreeSet<u64> = leases.iter().map(|l| l.2).collect();
        assert_eq!(pids.len(), 16, "sixteen distinct exit endpoints: {leases:?}");
        let mut running: BTreeSet<usize> = (1..=16).collect();
        for n in order {
            let (lease, budget, pid) = leases[n - 1];
            running.remove(&n);
            k.exit_taken(pid);
            k.expect_destroy = Some(budget);
            k.calls.clear();
            let object = s
                .exited(pid)
                .unwrap_or_else(|| panic!("agent-{n}'s exit endpoint {pid} names a running one"));
            assert_eq!(object.id, lease, "the pids map names agent-{n} at its own exit endpoint's report");
            s.event(&mut k, EventKind::Exited { object }, 0).unwrap();
            assert!(s.failed.is_empty(), "agent-{n}: {:?}", s.failed);
            let destroys = k.calls.iter().filter(|c| matches!(c, Call::Destroy(_))).count();
            assert_eq!(destroys, 1, "agent-{n}'s end is one destroy by handle: {:?}", k.calls);
            assert_eq!(k.expect_destroy, None, "agent-{n}'s destroy was made");
            assert!(!k.budgets.contains(&budget), "agent-{n}'s budget is gone");
            for m in &running {
                let (_, b, p) = leases[m - 1];
                assert!(k.budgets.contains(&b), "agent-{m}'s budget {b} survives agent-{n}'s end");
                assert!(s.exited(p).is_some_and(|o| o.id == leases[m - 1].0), "agent-{m} still runs");
            }
            assert!(k.calls.iter().any(|c| matches!(c, Call::Notify { lease: l, .. } if *l == lease)));
        }
    }
}

/// The scope made in `budget`: its one child with no deadline of its own.
fn scope_of(k: &Recorder, budget: usize) -> usize {
    let scopes = k.deadlines.iter().filter(|d| d.1 == FOREVER).map(|d| d.0);
    scopes.filter(|s| k.parents.get(s) == Some(&budget)).last().expect("the budget's scope")
}

/// Runs a batch of the test's own, one no launch stops.
fn ran(s: &mut Steward<usize, usize>, k: &mut Recorder, batch: Batch) -> Result<(), ()> {
    match s.run(k, batch) {
        Ran::Done(result) => result.map(|_| ()).map_err(|_| ()),
        Ran::Pending(_) => panic!("a batch with no launch step"),
    }
}

/// A `DestroyBudget` of `owner`'s token at `slot`, as a batch of its own: the core never asks
/// for a scope's, so the steps here are the test's.
fn destroy_step(owner: &Object, slot: u8) -> Batch {
    Batch {
        owner: owner.clone(),
        steps: vec![Step::DestroyBudget { budget: Token { owner: owner.clone(), slot } }],
    }
}

/// A scope is destroyed by its handle as a budget is, with its parent's deadline the witness,
/// since it was minted with none and the kernel takes it with its parent: a lease the deadline
/// took, its scope's index recycled for a new lease's scope before the notice, makes no
/// destroy by it, and the new scope is live in the kernel after (the verdict); ahead of the
/// deadline a scope's destroy is by its handle, as before. The core names only budgets in its
/// `DestroyBudget`s today, so the scope's step is the test's own batch.
#[test]
fn a_scopes_index_recycled_after_its_leases_deadline_is_not_destroyed_as_it() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let (_, budget, pid) = lease_for(&mut s, &mut k, u, 2_000_000);
    let scope = scope_of(&k, budget);
    let destroy_scope = destroy_step(&s.exited(pid).unwrap(), SCOPE);
    k.clock += 2_000_000;
    k.deadline_takes(budget);
    let (_, recycled, _) = lease_for(&mut s, &mut k, u, 60_000_000);
    assert_eq!(recycled, budget, "the new lease's budget took the freed index");
    assert_eq!(scope_of(&k, recycled), scope, "and its scope the old scope's");
    k.calls.clear();
    ran(&mut s, &mut k, destroy_scope).unwrap();
    assert!(k.budgets.contains(&scope), "the new lease's scope survives");
    assert!(!k.calls.iter().any(|c| matches!(c, Call::Destroy(_))), "{:?}", k.calls);
    let (_, budget, pid) = lease_for(&mut s, &mut k, u, 2_000_000);
    let scope = scope_of(&k, budget);
    k.calls.clear();
    let live = destroy_step(&s.exited(pid).unwrap(), SCOPE);
    ran(&mut s, &mut k, live).unwrap();
    assert!(k.calls.contains(&Call::Destroy(scope)), "{:?}", k.calls);
    assert!(!k.budgets.contains(&scope), "ahead of the deadline the scope is destroyed by its handle");
    assert!(s.failed.is_empty(), "{:?}", s.failed);
}

/// The steward's own destroy of a budget takes its scope with it too, its index freed for the
/// next handle, which a new lease's budget takes: a destroy of the scope asked for after that,
/// its parent's record gone, makes no call by its handle, and the budget at the index is live
/// after (the verdict).
#[test]
fn a_scopes_index_recycled_after_its_budgets_destroy_is_not_destroyed_as_it() {
    // The batch's budget slot, `BUDGET` in the core's effects.rs, which is its own.
    const BUDGET: u8 = 0;
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let (_, budget, pid) = lease_for(&mut s, &mut k, u, 60_000_000);
    let scope = scope_of(&k, budget);
    let owner = s.exited(pid).unwrap();
    ran(&mut s, &mut k, destroy_step(&owner, BUDGET)).unwrap();
    assert!(k.calls.contains(&Call::Destroy(budget)), "{:?}", k.calls);
    k.destroy_took(budget);
    let (_, recycled, _) = lease_for(&mut s, &mut k, u, 60_000_000);
    assert_eq!(recycled, scope, "the new lease's budget took the scope's index");
    k.calls.clear();
    ran(&mut s, &mut k, destroy_step(&owner, SCOPE)).unwrap();
    assert!(k.budgets.contains(&recycled), "the new lease's budget survives");
    assert!(!k.calls.iter().any(|c| matches!(c, Call::Destroy(_))), "{:?}", k.calls);
    assert!(s.failed.is_empty(), "{:?}", s.failed);
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

/// The first budget made since `before` (the batch's own, whose destruction takes the scope
/// made inside it) was destroyed since.
fn made_and_destroyed(k: &Recorder, before: usize) -> bool {
    let (first, _) = k.deadlines[before];
    k.calls.iter().any(|c| matches!(c, Call::Destroy(d) if *d == first))
}

/// A mint the server does not take within the steward's bound (`SERVER_US`, `Timeout`) fails
/// the login's step and nothing else (servers/steward.md, "Residual risks"): the batch is rolled
/// back, every budget it made destroyed, the step is said with the kernel's error, and the next
/// caller is served. A `Timeout` is no answer: the step parked at that server was woken before
/// the mint, as before any mint there, and is not woken again by the timeout; it runs again only
/// when the server answers.
#[test]
fn a_mint_the_server_does_not_take_in_time_fails_the_step_and_the_next_caller_is_served() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let bobs = parked_at(&mut s, &mut k, "erofsd:system");
    let before = k.deadlines.len();
    k.stalled = vec!["erofsd:system".into()];
    let e = call(&mut s, &mut k, SSHD, login("alice", "", &ALICE_KEY)).unwrap_err();
    assert_eq!(e, ErrorCode::Failed, "the step the bound failed fails the login");
    assert!(
        matches!(core::mem::take(&mut s.failed).as_slice(), [(_, Error::Timeout)]),
        "one failed step, with the kernel's error"
    );
    assert!(k.deadlines.len() > before, "the batch got as far as its budgets");
    assert!(made_and_destroyed(&k, before), "every budget the batch made is destroyed: {:?}", k.calls);
    assert!(cleaned(&k).is_empty(), "the loop made no cleanup call: {:?}", k.calls);
    assert_eq!(woken(&k), ["erofsd:system"], "woken before the mint; a timeout wakes nothing: {:?}", k.calls);
    assert_eq!(mints_at_system(&k), 1, "not retried");
    k.calls.clear();
    let fates = k.thread();
    assert_eq!(
        cleaned(&k).iter().filter(|(h, _)| *h == bobs).count(),
        1,
        "the woken step ran once: {:?}",
        k.calls
    );
    assert!(fates.contains(&(bobs, Fate::Parked(TRIES + 1))), "and is parked again: {fates:?}");
    k.calls.clear();
    assert!(k.thread().is_empty(), "not woken again by the timeout: {:?}", k.calls);
    assert_eq!(k.parked(), 1, "still parked");
    k.stalled.clear();
    k.busy.clear();
    k.calls.clear();
    assert!(
        matches!(call(&mut s, &mut k, SSHD, bob_in("")), Ok(Reply::Login(_))),
        "the next caller is served"
    );
    k.thread();
    assert_eq!((k.pending(), k.parked()), (0, 0), "the server's answer ran bob's step: {:?}", k.calls);
}

/// A launch whose read of the image the server does not take in time fails the lease's step the
/// same way: `start_agent` answers `failed`, the lease's budgets are destroyed, and the sponsor's
/// next `start_agent` is served.
#[test]
fn a_launch_whose_image_read_is_not_taken_in_time_fails_start_agent() {
    let mut k = Recorder::default();
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    let before = k.deadlines.len();
    k.stalled = vec!["bootfsd".into()];
    let e = call(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 })).unwrap_err();
    assert_eq!(e, ErrorCode::Failed);
    assert!(matches!(core::mem::take(&mut s.failed).as_slice(), [(_, Error::Timeout)]));
    assert!(k.deadlines.len() > before && made_and_destroyed(&k, before), "{:?}", k.calls);
    k.stalled.clear();
    k.calls.clear();
    assert!(matches!(
        call(&mut s, &mut k, u, Message::StartAgent(StartAgent { lease: 60_000_000 })),
        Ok(Reply::StartAgent(_))
    ));
}

/// A push whose source's server does not take the steward's `stat` in time answers `failed`
/// before any budget is carved; the same push is served when the server answers again.
#[test]
fn a_push_whose_source_server_does_not_answer_in_time_answers_failed() {
    let mut k = Recorder::default();
    k.homes.insert(("alice".into(), "in".into()), vec![0xA5u8; 64]);
    let mut s = started(&mut k);
    let u = alice_session(&mut s, &mut k);
    open_channel(&mut s, &mut k, &ALICE_APPROVAL_KEY).unwrap();
    let labels = label_bytes(&[7]);
    k.stalled = vec!["walfsd:data".into()];
    k.calls.clear();
    let e = call(&mut s, &mut k, u, submit(3, &labels, 0, "in\nsecret/in", "")).unwrap_err();
    assert_eq!(e, ErrorCode::Failed);
    assert!(k.creates().is_empty() && s.failed.is_empty(), "refused before any batch: {:?}", k.calls);
    k.stalled.clear();
    assert!(matches!(
        call(&mut s, &mut k, u, submit(3, &labels, 0, "in\nsecret/in", "")),
        Ok(Reply::Submit(_))
    ));
}
