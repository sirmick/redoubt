//! Multiplexed connections on the fake kernel's held clock (servers/serving.md, "Multiplexed
//! connections"; R77): a completion call's answer that took the file server longer than the
//! session bound leaves the session its whole bound from that answer, and a request behind the
//! slow one is still served: a request not yet tried is tried, whatever the clock says. A session
//! answered after another's slow fill in the same pass has its whole bound too. A `Tversion` by
//! one call ends its key's session, as init's restart of an entry depends on: no tick can end the
//! session first. The clock is the binary's: one test at a time.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::client::{Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Caller, Request, Words};
use redoubt_rt::server::ninep::{
    Around, ENDED, FileServer, FileStat, NineError, NineServer, OPENED, Qid, Read, Write, collect_words,
    mode, refuse_malformed,
};
use redoubt_rt::server::{AdmitKey, Limits, Resource};
use redoubt_rt::wire::ninep::{Body, Message, message_size};

/// The session bound, and how long a request waits: the server's `requests_wait`.
const BOUND: u64 = 1_000_000;
/// How long a write to `slow` holds the server on the held clock: twice the bound, as walfsd
/// away on its device under load was (an 11 s fill against its 10 s bound).
const SLOW_DISPATCH: u64 = 2 * BOUND;

/// How long a completion call may be held, within the bound: a request's deadline is not
/// reached by its end.
const HOLD: u64 = BOUND / 2;

/// A root holding `now`, whose reads are answered at once, `slow`, whose writes move the held
/// clock [`SLOW_DISPATCH`] before they are answered, and `held`, whose first read waits and whose
/// next moves the clock as `slow`'s writes do, and `wait`, whose reads always wait. `tries` counts
/// `held`'s reads.
struct Files {
    tries: u32,
}

const ROOT: u8 = 0;
const NOW: u8 = 1;
const SLOW: u8 = 2;
const HELD: u8 = 3;
const WAIT: u8 = 4;

fn qid(node: u8) -> Qid { Qid { kind: if node == ROOT { 0x80 } else { 0 }, version: 0, path: node.into() } }

impl FileServer for Files {
    type Node = u8;

    fn attach(&mut self, _: &Caller, _: &str) -> Result<(u8, Qid), NineError> { Ok((ROOT, qid(ROOT))) }

    fn labels(&self, _: &u8) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, _: &u8, name: &str) -> Result<(u8, Qid), NineError> {
        let node = match name {
            "now" => NOW,
            "slow" => SLOW,
            "held" => HELD,
            "wait" => WAIT,
            _ => return Err(NineError::NOT_FOUND),
        };
        Ok((node, qid(node)))
    }

    fn open(&mut self, _: &Caller, node: &u8, _: u8) -> Result<Qid, NineError> { Ok(qid(*node)) }

    fn read(&mut self, _: &Caller, node: &u8, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        if *node == WAIT {
            return Ok(Read::Wait);
        }
        if *node == HELD {
            self.tries += 1;
            if self.tries == 1 {
                return Ok(Read::Wait);
            }
            fake().tick(SLOW_DISPATCH);
        }
        let data = b"hello".get(offset as usize..).unwrap_or(&[]);
        let n = data.len().min(out.len());
        out[..n].copy_from_slice(&data[..n]);
        Ok(Read::Done(n))
    }

    fn write(&mut self, _: &Caller, _: &u8, _: u64, data: &[u8]) -> Result<usize, NineError> {
        Ok(data.len())
    }

    fn write_or_wait(&mut self, _: &Caller, node: &u8, _: u64, data: &[u8]) -> Result<Write, NineError> {
        if *node == SLOW {
            // The server's own thread moves the clock, once the client waits in its call.
            fake().tick(SLOW_DISPATCH);
        }
        Ok(Write::Done(data.len()))
    }

    fn stat(&mut self, _: &Caller, node: &u8) -> Result<FileStat, NineError> {
        Ok(FileStat { qid: qid(*node), ..FileStat::default() })
    }

    fn dir_entry(&mut self, _: &Caller, _: &u8, _: u64) -> Result<Option<(u8, FileStat)>, NineError> {
        Ok(None)
    }
}

const LIMITS: Limits = Limits { buckets: 4, in_flight: 8, files: 8, state: 0, requests: 6, pages: 2 };

const NOW_FID: u32 = 1;
const SLOW_FID: u32 = 2;
const HELD_FID: u32 = 3;
const WAIT_FID: u32 = 4;

/// A test's turn: the clock is the binary's, and a tick steps every test's deadlines at once, so
/// the tests run one at a time.
fn turn() -> MutexGuard<'static, ()> {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let turn = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    fake().hold_clock();
    turn
}

/// What the server holds, published at each turn before it waits: by account, its session's
/// completion call (`InFlight`), its requests (`Requests`) and their pages (`Pages`); and the
/// sessions open.
#[derive(Default)]
struct Seen {
    held: Mutex<HashMap<u64, u32>>,
    sessions: AtomicUsize,
}

impl Seen {
    fn held(&self, account: u64) -> u32 { self.held.lock().unwrap().get(&account).copied().unwrap_or(0) }

    fn sessions(&self) -> usize { self.sessions.load(Ordering::Acquire) }
}

/// Beside the skeleton's loop: the accounts the server has seen by their calls, and what each
/// holds, published at each turn.
struct Publish {
    seen: Arc<Seen>,
    keys: HashMap<u64, AdmitKey>,
}

impl Around<Files> for Publish {
    fn call(&mut self, nine: &mut NineServer<Files>, request: Request, _: u64) {
        self.keys.entry(request.caller.account).or_insert(nine.charge_of(&request.caller).0);
        let _ = nine.serve_with(request, |_, r| refuse_malformed(r).map(|()| None));
    }

    fn turn(&mut self, nine: &mut NineServer<Files>, _: u64) {
        let mut held = self.seen.held.lock().unwrap();
        for (account, key) in &self.keys {
            let of = |r| nine.admission().held(*key, r);
            held.insert(*account, of(Resource::InFlight) + of(Resource::Requests) + of(Resource::Pages));
        }
        self.seen.sessions.store(nine.sessions(), Ordering::Release);
    }
}

/// The server, its receive endpoint, its thread, multiplexing with [`BOUND`], and what it holds.
fn start() -> (usize, Handle, std::thread::JoinHandle<u32>, Arc<Seen>) {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let seen = Arc::new(Seen::default());
    let publish = Publish { seen: seen.clone(), keys: HashMap::new() };
    let thread = f.run(server, move || {
        let mut nine = NineServer::new(Files { tries: 0 }, LIMITS, 7).unwrap();
        nine.requests_wait(BOUND);
        nine.run_around(&Endpoint::from_handle(receive), publish)
    });
    (server, receive, thread, seen)
}

/// The files a client opens: `now`, `slow` and `held`; three working fids are what a badge's
/// share of [`LIMITS`] holds beside its root.
const FILES: &[(u32, &str)] = &[(NOW_FID, "now"), (SLOW_FID, "slow"), (HELD_FID, "held")];
/// The files the instances of a restarted entry open: `now` and `wait`.
const RESTARTED: &[(u32, &str)] = &[(NOW_FID, "now"), (WAIT_FID, "wait")];

/// A client of `account` on its own badge: `files` open by one-call 9P, then its session.
fn client(server: usize, receive: Handle, account: u64, files: &[(u32, &str)]) -> (usize, Handle) {
    let f = fake();
    let pid = f.process(account, &[]);
    let ep = f.grant(server, receive, pid, account);
    f.as_process(pid, || {
        open_files(ep, files);
        assert_eq!(collect_held(ep, 0).0, [0, 0, OPENED, 0], "the session opens");
    });
    (pid, ep)
}

/// A new instance on `account`'s badge, as init's restart of an entry hands it the badge its
/// predecessor had, and its `Tversion`, by one call.
fn successor(server: usize, receive: Handle, account: u64) -> (usize, Handle) {
    let f = fake();
    let pid = f.process(account, &[]);
    let ep = f.grant(server, receive, pid, account);
    f.as_process(pid, || {
        let c = Connection::new(Endpoint::from_handle(ep));
        c.version(&mut Lend::new(1).unwrap()).unwrap();
    });
    (pid, ep)
}

/// Attaches by one-call 9P and opens each of `files` on its fid.
fn open_files(ep: Handle, files: &[(u32, &str)]) {
    let c = Connection::new(Endpoint::from_handle(ep));
    let mut lend = Lend::new(1).unwrap();
    c.attach(&mut lend, 0, "").unwrap();
    for &(fid, name) in files {
        c.walk(&mut lend, 0, fid, name).unwrap();
        c.open(&mut lend, fid, mode::ORDWR).unwrap();
    }
}

/// Waits for `cond`, failing naming `step` well before the fake kernel's own 60 s guard.
fn until(step: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..20_000 {
        if cond() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("{step}: not reached");
}

/// A completion call the server may hold `hold` µs: its reply words and the answers' bytes.
fn collect_held(ep: Handle, hold: u64) -> (Words, Vec<u8>) {
    let outcome =
        Endpoint::from_handle(ep).call(&collect_words(hold), &[], Some(Buffer::new(4).unwrap()), FOREVER);
    let (reply, buffer) = outcome.into_result().unwrap();
    let n = (reply.words[1] as usize).min(buffer.as_ref().map_or(0, |b| b.len()));
    (reply.words, buffer.map_or_else(Vec::new, |b| b[..n].to_vec()))
}

/// Sends one request in a page of its own.
fn send(ep: Handle, tag: u16, body: Body<'_>) {
    let mut pages = Buffer::new(1).unwrap();
    let n = Message { tag, body }.encode(&mut pages).unwrap();
    Endpoint::from_handle(ep)
        .send(&[0, n as u64, 0, 0], &[], Some(pages), FOREVER)
        .map_err(|(e, _)| e)
        .unwrap();
}

/// The R-messages of a completion call's answer: each tag, and its count or error.
fn answers(mut bytes: &[u8]) -> Vec<(u16, Result<u32, String>)> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let size = message_size(bytes).unwrap();
        let message = Message::decode(&bytes[..size]).unwrap();
        out.push(match message.body {
            Body::Rwrite { count } => (message.tag, Ok(count)),
            Body::Rread { data } => (message.tag, Ok(data.len() as u32)),
            Body::Rerror { ename } => (message.tag, Err(ename.to_string())),
            other => panic!("unexpected answer {other:?}"),
        });
        bytes = &bytes[size..];
    }
    out
}

#[test]
fn a_fill_longer_than_the_bound_leaves_the_session_its_whole_bound_from_the_answer() {
    let _turn = turn();
    let f = fake();
    let (server, receive, thread, _) = start();
    let (a, ha) = client(server, receive, 1001, FILES);
    f.as_process(a, || {
        // A write to `slow`, then a read whose deadline passes while the write is served: never
        // tried, it is served in its turn, not answered `timeout`.
        send(ha, 0, Body::Twrite { fid: SLOW_FID, offset: 0, data: b"x" });
        send(ha, 1, Body::Tread { fid: NOW_FID, offset: 0, count: 64 });
        let (words, bytes) = collect_held(ha, FOREVER);
        assert_eq!(words[0], 0, "the completion call answered");
        assert_eq!(
            answers(&bytes),
            vec![(0, Ok(1)), (1, Ok(5))],
            "the read behind the slow write was not served"
        );
    });
    // Just short of the bound from the answer: the session is the one the answer went to, so a
    // call is parked and answered empty at its hold of 0, not opened afresh. A bound counted from
    // before the fill had ended the session at the answer.
    f.tick(BOUND - 1);
    f.as_process(a, || assert_eq!(collect_held(ha, 0).0, [0, 0, 0, 0], "the session ended before its bound"));
    // The whole bound from that answer with no call parked: the session ends, and the next call
    // opens a new one. The second tick waits for the server's receive again, after the first's
    // expiry.
    f.tick(BOUND);
    f.tick(0);
    f.as_process(a, || {
        assert_eq!(collect_held(ha, 0).0, [0, 0, OPENED, 0], "the session outlived its bound")
    });
    until("the session's calls are answered", || f.open_calls(server) == 0);
    f.destroy(server, receive);
    assert_eq!(thread.join().unwrap(), 0);
}

/// Two sessions due at one turn: A's fill serves its held read, which takes the server twice the
/// bound, then B's call is answered empty at its hold. B's bound runs from its own answer, not
/// from the turn's time, which A's fill left stale.
#[test]
fn a_session_answered_after_a_slow_fill_in_the_same_pass_has_its_whole_bound() {
    let _turn = turn();
    let f = fake();
    let (server, receive, thread, _) = start();
    let (a, ha) = client(server, receive, 1001, FILES);
    let (b, hb) = client(server, receive, 1002, FILES);
    // A parks a call held HOLD; its read of `held` is tried and waits.
    let a_call = f.run(a, move || {
        let (words, bytes) = collect_held(ha, HOLD);
        u32::from(words[0] == 0 && answers(&bytes) == vec![(0, Ok(5))])
    });
    until("A's call is parked", || f.open_calls(server) == 1);
    f.as_process(a, || send(ha, 0, Body::Tread { fid: HELD_FID, offset: 0, count: 64 }));
    // B parks a call held as long, with nothing to answer.
    let b_call = f.run(b, move || u32::from(collect_held(hb, HOLD).0 == [0, 0, 0, 0]));
    until("B's call is parked", || f.open_calls(server) == 2);
    // Both holds run out at one turn: A first, its read served at last, then B, empty.
    f.tick(HOLD);
    assert_eq!(a_call.join().unwrap(), 1, "A's held read was not answered");
    assert_eq!(b_call.join().unwrap(), 1, "B's call was not answered empty");
    // Another event, A's next request, gives the server a turn, which judges B's session; the
    // tick of nothing waits until that turn is over.
    f.as_process(a, || send(ha, 1, Body::Tread { fid: NOW_FID, offset: 0, count: 64 }));
    f.tick(0);
    // B's session is the one its answer went to: a call is parked on it, not opened afresh.
    f.as_process(b, || {
        assert_eq!(collect_held(hb, 0).0, [0, 0, 0, 0], "B's session ended at its own answer")
    });
    until("the sessions' calls are answered", || f.open_calls(server) == 0);
    f.destroy(server, receive);
    assert_eq!(thread.join().unwrap(), 0);
}

/// A `Tversion` by one call ends everything its connection key holds, the multiplexed session
/// with its fids: init re-mints the same badge for an entry it restarts, and the successor's
/// `Tversion` is where its predecessor's session goes, its parked call answered ended, its
/// requests and admission with it, so the successor's first completion call opens a session of its
/// own and collects nothing of the predecessor's. The clock is held: the session bound never ends
/// the session first. A `Tversion` in a session ends it too.
#[test]
fn a_tversion_by_one_call_ends_the_keys_session() {
    let _turn = turn();
    let f = fake();
    let (server, receive, thread, seen) = start();
    let read = |ep, tag, fid| send(ep, tag, Body::Tread { fid, offset: 0, count: 64 });
    // The first instance: a parked completion call, and three reads that wait (its share).
    let (a, ha) = client(server, receive, 1001, RESTARTED);
    let parked = f.run(a, move || u32::from(collect_held(ha, FOREVER).0[0] == u64::from(ENDED)));
    until("A's call is parked", || f.open_calls(server) == 1);
    f.as_process(a, || (0..3).for_each(|tag| read(ha, tag, WAIT_FID)));
    until("A's call and requests are held", || seen.held(1001) == 4);
    // The next instance on the same badge versions the connection: A's session ends.
    let (b, hb) = successor(server, receive, 1001);
    until("A's parked call is answered", || parked.is_finished());
    assert_eq!(parked.join().unwrap(), 1, "A's parked call was not answered ended");
    until("nothing of A's session is held", || seen.held(1001) == 0 && seen.sessions() == 0);
    // B's session, with answers pending and no call parked: a read answered at once and one that
    // waits, both untried.
    f.as_process(b, || {
        open_files(hb, RESTARTED);
        assert_eq!(collect_held(hb, 0).0, [0, 0, OPENED, 0], "B's first completion call opens its session");
        read(hb, 0, NOW_FID);
        read(hb, 1, WAIT_FID);
    });
    until("B's requests are held", || seen.held(1001) == 3);
    // The next instance versions: its first completion call opens a session, empty, nothing of
    // B's in it, and a request on B's tag is its own.
    let (c, hc) = successor(server, receive, 1001);
    until("nothing of B's session is held", || seen.held(1001) == 0 && seen.sessions() == 0);
    f.as_process(c, || {
        assert_eq!(collect_held(hc, 0), ([0, 0, OPENED, 0], vec![]), "C met B's session");
        open_files(hc, RESTARTED);
        read(hc, 0, NOW_FID);
        let (words, bytes) = collect_held(hc, 0);
        assert_eq!(words[0], 0, "C's completion call answered");
        assert_eq!(answers(&bytes), vec![(0, Ok(5))], "C's read was not its own");
        // A `Tversion` in the session ends it.
        send(hc, 1, Body::Tversion { msize: 8192, version: "9P2000" });
    });
    until("a Tversion in the session ended it", || seen.held(1001) == 0 && seen.sessions() == 0);
    until("the sessions' calls are answered", || f.open_calls(server) == 0);
    f.destroy(server, receive);
    assert_eq!(thread.join().unwrap(), 0);
}
