//! Multiplexed connections on the fake kernel (servers/serving.md, "Multiplexed connections";
//! R77): requests by `send`, completions through one long-poll call, every attack's verdict read
//! from the server's own admission table and sessions, or from what the server answered.

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, FOREVER, Handle};
use redoubt_rt::client::{Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Caller, Request, Words};
use redoubt_rt::server::ninep::{
    Around, COLLECT_WAIT, ENDED, FileServer, FileStat, IN_WORDS, MALFORMED, NineError, NineServer, OPENED,
    Qid, Read, collect_words, mode, refuse_malformed,
};
use redoubt_rt::server::{AdmitKey, Limits, Resource};
use redoubt_rt::wire::ninep::{Body, Message, message_size};

/// A root holding `now`, whose reads are answered at once, and `wait`, whose reads always wait.
struct Files;

const ROOT: u8 = 0;
const NOW: u8 = 1;
const WAIT: u8 = 2;

fn qid(node: u8) -> Qid { Qid { kind: if node == ROOT { 0x80 } else { 0 }, version: 0, path: node.into() } }

impl FileServer for Files {
    type Node = u8;

    fn attach(&mut self, _: &Caller, _: &str) -> Result<(u8, Qid), NineError> { Ok((ROOT, qid(ROOT))) }

    fn labels(&self, _: &u8) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, _: &u8, name: &str) -> Result<(u8, Qid), NineError> {
        let node = match name {
            "now" => NOW,
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
        let data = b"hello".get(offset as usize..).unwrap_or(&[]);
        let n = data.len().min(out.len());
        out[..n].copy_from_slice(&data[..n]);
        Ok(Read::Done(n))
    }

    fn write(&mut self, _: &Caller, _: &u8, _: u64, data: &[u8]) -> Result<usize, NineError> {
        Ok(data.len())
    }

    fn stat(&mut self, _: &Caller, node: &u8) -> Result<FileStat, NineError> {
        Ok(FileStat { qid: qid(*node), ..FileStat::default() })
    }

    fn dir_entry(&mut self, _: &Caller, _: &u8, _: u64) -> Result<Option<(u8, FileStat)>, NineError> {
        Ok(None)
    }
}

/// What the server holds, published after every event it handles.
#[derive(Default)]
struct Seen {
    /// What each client account holds now, and the most it ever held: its session's completion call
    /// (`InFlight`), its requests (`Requests`) and their pages (`Pages`).
    held: Mutex<HashMap<u64, (u32, u32)>>,
    sessions: AtomicUsize,
}

impl Seen {
    fn held(&self, account: u64) -> u32 { self.held.lock().unwrap().get(&account).map_or(0, |h| h.0) }

    fn peak(&self, account: u64) -> u32 { self.held.lock().unwrap().get(&account).map_or(0, |h| h.1) }
}

/// The server's caps: four buckets of eight `InFlight`, six `Requests` and two `Pages`, so a lone
/// badge of a non-zero account holds its session's completion call, three requests and one page.
const LIMITS: Limits = Limits { buckets: 4, in_flight: 8, files: 8, state: 0, requests: 6, pages: 2 };
const SHARE: u32 = 4;
/// How long a request, or a session without a completion call, lasts.
const LONGEST: u64 = 300_000;

/// Beside the skeleton's loop: the accounts the server has seen, by the calls that open their
/// files, and what each holds, published at each turn.
struct Publish {
    seen: Arc<Seen>,
    keys: HashMap<u64, AdmitKey>,
}

impl Around<Files> for Publish {
    fn call(&mut self, nine: &mut NineServer<Files>, request: Request, _: u64) {
        self.keys.entry(request.caller.account).or_insert(nine.charge_of(&request.caller).0);
        let _ = nine.serve_with(request, |_, r| refuse_malformed(r));
    }

    /// Published before each wait, after the deadlines, so what the last event or deadline did is
    /// seen.
    fn turn(&mut self, nine: &mut NineServer<Files>, _: u64) {
        let mut held = self.seen.held.lock().unwrap();
        for (account, key) in &self.keys {
            let of = |r| nine.admission().held(*key, r);
            let now = of(Resource::InFlight) + of(Resource::Requests) + of(Resource::Pages);
            let entry = held.entry(*account).or_default();
            *entry = (now, entry.1.max(now));
        }
        self.seen.sessions.store(nine.sessions(), Ordering::Release);
    }

    fn abandoned(&mut self, _: &mut NineServer<Files>, _: NonZeroU64) {
        panic!("a notice for no completion call");
    }
}

fn serve(ep: Endpoint, seen: Arc<Seen>) -> u32 {
    let mut nine = NineServer::new(Files, LIMITS, 7).unwrap();
    nine.requests_wait(LONGEST);
    nine.run_around(&ep, Publish { seen, keys: HashMap::new() })
}

/// Waits for `cond`, failing naming `step` well before the fake kernel's own 60 s guard.
fn until(step: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !cond() {
        assert!(Instant::now() < deadline, "{step}: not reached");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The fids every client opens by one-call 9P before it multiplexes: `now` and `wait`.
const NOW_FID: u32 = 1;
const WAIT_FID: u32 = 2;

fn open_files(ep: Handle) {
    let c = Connection::new(Endpoint::from_handle(ep));
    let mut lend = Lend::new(1).unwrap();
    c.attach(&mut lend, 0, "").unwrap();
    for (fid, name) in [(NOW_FID, "now"), (WAIT_FID, "wait")] {
        c.walk(&mut lend, 0, fid, name).unwrap();
        c.open(&mut lend, fid, mode::OREAD).unwrap();
    }
}

/// A completion call: its status and reply words, or an error.
fn collect_raw(ep: Handle, timeout: u64) -> Result<(Words, Vec<u8>), Error> {
    collect_held(ep, COLLECT_WAIT, timeout)
}

/// A completion call the server may hold `hold` µs.
fn collect_held(ep: Handle, hold: u64, timeout: u64) -> Result<(Words, Vec<u8>), Error> {
    let words = collect_words(hold);
    let outcome = Endpoint::from_handle(ep).call(&words, &[], Some(Buffer::new(4).unwrap()), timeout);
    let (reply, buffer) = outcome.into_result()?;
    let n = (reply.words[1] as usize).min(buffer.as_ref().map_or(0, |b| b.len()));
    Ok((reply.words, buffer.map_or_else(Vec::new, |b| b[..n].to_vec())))
}

/// Opens the connection's session.
fn open_session(ep: Handle) {
    assert_eq!(collect_raw(ep, FOREVER).unwrap().0, [0, 0, OPENED, 0], "the session opens");
}

/// One answer, as the client sees it.
#[derive(Debug, PartialEq, Eq)]
enum Got {
    Data(Vec<u8>),
    Error(String),
    Flushed,
}

/// The R-messages of a completion call's reply, by tag.
fn answers(bytes: &[u8]) -> Vec<(u16, Got)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let size = message_size(&bytes[at..]).expect("an R-message frames");
        let message = Message::decode(&bytes[at..at + size]).expect("an R-message decodes");
        let got = match message.body {
            Body::Rread { data } => Got::Data(data.to_vec()),
            Body::Rerror { ename } => Got::Error(ename.to_string()),
            Body::Rflush => Got::Flushed,
            other => panic!("unexpected answer {other:?}"),
        };
        out.push((message.tag, got));
        at += size;
    }
    out
}

/// Collects once and returns its answers.
fn collect(ep: Handle) -> Vec<(u16, Got)> {
    let (words, bytes) = collect_raw(ep, FOREVER).unwrap();
    assert_eq!(words[0], 0, "a completion call answered");
    answers(&bytes)
}

/// Sends one request: in the words if it fits, else in a transfer.
fn send(ep: Handle, tag: u16, body: Body<'_>) {
    let mut bytes = vec![0; 4096];
    let n = Message { tag, body }.encode(&mut bytes).unwrap();
    let ep = Endpoint::from_handle(ep);
    if n <= IN_WORDS {
        let mut words = [0u64; 4];
        for (word, chunk) in words[1..].iter_mut().zip(bytes[..IN_WORDS].chunks(8)) {
            *word = u64::from_le_bytes(chunk.try_into().unwrap());
        }
        ep.send(&words, &[], None, FOREVER).unwrap();
    } else {
        let mut pages = Buffer::new(1).unwrap();
        pages[..n].copy_from_slice(&bytes[..n]);
        ep.send(&[0, n as u64, 0, 0], &[], Some(pages), FOREVER).map_err(|(e, _)| e).unwrap();
    }
}

fn read(ep: Handle, tag: u16, fid: u32) { send(ep, tag, Body::Tread { fid, offset: 0, count: 64 }) }

/// Sends reads of `fid` tagged `tags`, end to end in one transfer of one page.
fn reads_in_one_transfer(ep: Handle, tags: &[u16], fid: u32) { reads_in_pages(ep, tags, fid, 1) }

/// Sends reads of `fid` tagged `tags`, end to end in one transfer of `npages` pages.
fn reads_in_pages(ep: Handle, tags: &[u16], fid: u32, npages: usize) {
    let mut pages = Buffer::new(npages).unwrap();
    let mut n = 0;
    for tag in tags {
        n += Message { tag: *tag, body: Body::Tread { fid, offset: 0, count: 64 } }
            .encode(&mut pages[n..])
            .unwrap();
    }
    Endpoint::from_handle(ep)
        .send(&[0, n as u64, 0, 0], &[], Some(pages), FOREVER)
        .map_err(|(e, _)| e)
        .unwrap();
}

fn busy() -> Got { Got::Error(NineError::BUSY.0.to_string()) }

/// A server, and clients of accounts 1001 and up on badges 1 and up.
struct World {
    server: usize,
    receive: Handle,
    thread: std::thread::JoinHandle<u32>,
    seen: Arc<Seen>,
}

impl World {
    fn new() -> World {
        let f = fake();
        let server = f.process(0, &[]);
        let receive = f.endpoint(server);
        let seen = Arc::new(Seen::default());
        let publish = seen.clone();
        let thread = f.run(server, move || serve(Endpoint::from_handle(receive), publish));
        World { server, receive, thread, seen }
    }

    /// A client of `account` on its own badge, its files open.
    fn client(&self, account: u64) -> (usize, Handle) {
        let f = fake();
        let pid = f.process(account, &[]);
        let ep = f.grant(self.server, self.receive, pid, account);
        f.as_process(pid, || open_files(ep));
        (pid, ep)
    }

    fn end(self) {
        fake().destroy(self.server, self.receive);
        assert_eq!(self.thread.join().unwrap(), 0);
    }
}

#[test]
fn a_never_polling_client_holds_only_its_share() {
    let w = World::new();
    let f = fake();
    let (a, ha) = w.client(1001);
    let (b, hb) = w.client(1002);
    // A opens a session and floods it with reads that wait, and never polls.
    f.as_process(a, || {
        open_session(ha);
        for tag in 0..10 {
            read(ha, tag, WAIT_FID);
        }
    });
    until("the server took A's flood", || w.seen.peak(1001) == SHARE);
    // B, beside it, is served both ways: by one call, and through a session of its own.
    f.as_process(b, || {
        let c = Connection::new(Endpoint::from_handle(hb));
        let mut lend = Lend::new(1).unwrap();
        let mut out = [0; 8];
        assert_eq!(c.read(&mut lend, NOW_FID, 0, &mut out), Ok(5));
        open_session(hb);
        read(hb, 0, NOW_FID);
        assert_eq!(collect(hb), vec![(0, Got::Data(b"hello".to_vec()))]);
    });
    // A held its share and no more: its completion call's slot and three requests.
    assert_eq!(w.seen.peak(1001), SHARE, "A held more than its share");
    // When A does poll, its excess is answered busy, in tag order; its three admitted reads wait.
    let excess: Vec<_> = (3..10).map(|tag| (tag, busy())).collect();
    f.as_process(a, || assert_eq!(collect(ha), excess));
    w.end();
}

#[test]
fn death_with_requests_parked_frees_the_connection() {
    let w = World::new();
    let f = fake();
    let (a, ha) = w.client(1003);
    f.as_process(a, || {
        open_session(ha);
        for tag in 0..3 {
            read(ha, tag, WAIT_FID);
        }
    });
    until("A's requests are held", || w.seen.held(1003) == 4);
    // A's completion call is parked, and A gives up on it: its abandonment ends the session.
    let caller = f.run(a, move || u32::from(collect_raw(ha, 200_000) == Err(Error::Timeout)));
    until("the completion call is parked", || f.open_calls(w.server) == 1);
    assert_eq!(caller.join().unwrap(), 1, "the completion call timed out");
    until("the session ended", || w.seen.sessions.load(Ordering::Acquire) == 0);
    assert_eq!(w.seen.held(1003), 0, "the session's admission was released");
    assert_eq!(f.open_calls(w.server), 0, "the abandoned call was answered");
    // The next completion call opens a fresh session: nothing of the old one is held.
    f.as_process(a, || {
        open_session(ha);
        read(ha, 0, NOW_FID);
        assert_eq!(collect(ha), vec![(0, Got::Data(b"hello".to_vec()))]);
    });
    w.end();
}

#[test]
fn a_death_between_completion_calls_is_found_at_the_session_bound() {
    let w = World::new();
    let f = fake();
    let (a, ha) = w.client(1004);
    // A sends a read answered at once and one that waits, and never calls again.
    let started = Instant::now();
    f.as_process(a, || {
        open_session(ha);
        read(ha, 0, NOW_FID);
        read(ha, 1, WAIT_FID);
    });
    until("A's requests are held", || w.seen.held(1004) == 3);
    until("the session ended at its deadline", || w.seen.sessions.load(Ordering::Acquire) == 0);
    assert!(started.elapsed() >= Duration::from_micros(LONGEST), "ended before its deadline");
    assert_eq!(w.seen.held(1004), 0, "every request's admission was released");
    w.end();
}

#[test]
fn a_flush_racing_a_completion_answers_once() {
    let w = World::new();
    let f = fake();
    let (a, ha) = w.client(1005);
    f.as_process(a, || {
        open_session(ha);
        // Flushed before any completion call: its read is never answered, only the Rflush.
        read(ha, 1, NOW_FID);
        send(ha, 2, Body::Tflush { oldtag: 1 });
        assert_eq!(collect(ha), vec![(2, Got::Flushed)]);
        // Answered first: the answer, then the Rflush alone.
        read(ha, 1, NOW_FID);
        assert_eq!(collect(ha), vec![(1, Got::Data(b"hello".to_vec()))]);
        send(ha, 2, Body::Tflush { oldtag: 1 });
        assert_eq!(collect(ha), vec![(2, Got::Flushed)]);
    });
    // A waiting read flushed while the completion call is parked: the Rflush, never the read.
    f.as_process(a, || read(ha, 3, WAIT_FID));
    let parked = f.run(a, move || {
        let got = collect(ha);
        u32::from(got == vec![(4, Got::Flushed)])
    });
    until("the completion call is parked", || f.open_calls(w.server) == 1);
    f.as_process(a, || send(ha, 4, Body::Tflush { oldtag: 3 }));
    assert_eq!(parked.join().unwrap(), 1, "the flush was answered, alone");
    // Its tag is free again, and nothing more comes for it.
    f.as_process(a, || {
        read(ha, 3, NOW_FID);
        assert_eq!(collect(ha), vec![(3, Got::Data(b"hello".to_vec()))]);
    });
    // What the server holds is published before its next receive, which follows its reply.
    until("only the session's slot is held", || w.seen.held(1005) == 1);
    w.end();
}

/// A request that comes late in a completion call's hold, and is answered into it, leaves the
/// session its whole bound from that answer, not from when the server began to wait (R77).
#[test]
fn a_request_late_in_a_hold_leaves_the_session_its_whole_bound() {
    const BOUND: u64 = 1_000_000;
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let thread = f.run(server, move || {
        let mut nine = NineServer::new(Files, LIMITS, 7).unwrap();
        nine.requests_wait(BOUND);
        nine.run(&Endpoint::from_handle(receive), |_, r| refuse_malformed(r))
    });
    let world = World { server, receive, thread, seen: Arc::default() };
    let (a, ha) = world.client(1008);
    f.as_process(a, || open_session(ha));
    let parked = f.run(a, move || u32::from(collect(ha) == vec![(0, Got::Data(b"hello".to_vec()))]));
    until("the completion call is parked", || f.open_calls(server) == 1);
    std::thread::sleep(Duration::from_micros(BOUND * 7 / 10));
    f.as_process(a, || read(ha, 0, NOW_FID));
    assert_eq!(parked.join().unwrap(), 1, "the read was answered into the parked call");
    // Past the bound from when the server began to wait; well within it from the answer.
    std::thread::sleep(Duration::from_micros(BOUND * 6 / 10));
    let again = f.run(a, move || u32::from(collect_held(ha, 0, FOREVER).unwrap().0 == [0, 0, 0, 0]));
    assert_eq!(again.join().unwrap(), 1, "the session ended early: the next call opened a new one");
    world.end();
}

#[test]
fn a_flood_of_sends_at_wait_cap_never_blocks_the_server() {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    // The skeleton's own loop.
    let thread = f.run(server, move || {
        let mut nine = NineServer::new(Files, LIMITS, 7).unwrap();
        nine.run(&Endpoint::from_handle(receive), |_, r| refuse_malformed(r))
    });
    let world = World { server, receive, thread, seen: Arc::default() };
    let (a, ha) = world.client(1006);
    let (b, hb) = world.client(1007);
    f.as_process(a, || open_session(ha));
    // Four threads of A send 256 requests at once, far past a group's WAIT_CAP (32), and never poll.
    let flood: Vec<_> = (0..4u16)
        .map(|t| f.run(a, move || (t * 64..t * 64 + 64).map(|tag| read(ha, tag, WAIT_FID)).count() as u32))
        .collect();
    // B is served meanwhile, call after call.
    let served = f.run(b, move || {
        let c = Connection::new(Endpoint::from_handle(hb));
        let mut lend = Lend::new(1).unwrap();
        let mut out = [0; 8];
        (0..64).filter(|_| c.read(&mut lend, NOW_FID, 0, &mut out) == Ok(5)).count() as u32
    });
    assert_eq!(served.join().unwrap(), 64, "B was not served throughout");
    for sender in flood {
        assert_eq!(sender.join().unwrap(), 64, "every send was taken");
    }
    // A's excess is answered busy; its three admitted reads still wait.
    f.as_process(a, || {
        let got = collect(ha);
        assert_eq!(got.len(), 253);
        assert!(got.iter().all(|(_, g)| *g == busy()));
    });
    world.end();
}

#[test]
fn a_reused_or_out_of_range_tag_ends_the_connection() {
    let w = World::new();
    let f = fake();
    let (a, ha) = w.client(1008);
    f.as_process(a, || {
        open_session(ha);
        read(ha, 5, WAIT_FID);
        read(ha, 5, WAIT_FID);
    });
    until("a reused tag ended the session", || w.seen.sessions.load(Ordering::Acquire) == 0);
    assert_eq!(w.seen.held(1008), 0);
    // An out-of-range tag ends it too, and its parked completion call is told.
    f.as_process(a, || {
        open_session(ha);
        read(ha, 0, WAIT_FID);
    });
    let parked =
        f.run(a, move || u32::from(collect_raw(ha, FOREVER).map(|r| r.0[0]) == Ok(u64::from(ENDED))));
    until("the completion call is parked", || f.open_calls(w.server) == 1);
    f.as_process(a, || read(ha, 256, NOW_FID));
    assert_eq!(parked.join().unwrap(), 1, "the parked call was answered ENDED");
    until("the session ended", || w.seen.sessions.load(Ordering::Acquire) == 0);
    assert_eq!(w.seen.held(1008), 0);
    // A request sent with no session open is dropped: nothing is held for it.
    f.as_process(a, || read(ha, 0, NOW_FID));
    f.as_process(a, || open_session(ha));
    until("the new session alone is held", || w.seen.held(1008) == 1);
    w.end();
}

#[test]
fn a_second_completion_call_is_refused() {
    let w = World::new();
    let f = fake();
    let (a, ha) = w.client(1009);
    f.as_process(a, || open_session(ha));
    let hello = || Got::Data(b"hello".to_vec());
    let first = f.run(a, move || u32::from(collect(ha) == vec![(7, hello()), (8, hello())]));
    until("the first completion call is parked", || f.open_calls(w.server) == 1);
    f.as_process(a, || {
        let second = collect_raw(ha, FOREVER).unwrap().0;
        assert_eq!(second, MALFORMED, "a second completion call is refused at once");
        // The first still holds the session: requests now are answered to it, these two sent
        // end to end in one transfer.
        reads_in_one_transfer(ha, &[7, 8], NOW_FID);
    });
    assert_eq!(first.join().unwrap(), 1, "the first completion call took the answer");
    w.end();
}

#[test]
fn a_completion_call_is_held_at_most_its_hold_and_the_servers_bound() {
    let w = World::new();
    let f = fake();
    let (a, ha) = w.client(1010);
    f.as_process(a, || {
        open_session(ha);
        // A hold of 0 is answered at once, and empty.
        let started = Instant::now();
        assert_eq!(collect_held(ha, 0, FOREVER).unwrap(), ([0; 4], vec![]));
        assert!(started.elapsed() < Duration::from_secs(5), "a hold of 0 waited");
        // With nothing to answer, a call is answered empty when its hold runs out.
        let started = Instant::now();
        assert_eq!(collect_held(ha, 100_000, FOREVER).unwrap(), ([0; 4], vec![]));
        let held = started.elapsed();
        assert!(held >= Duration::from_millis(100) && held < Duration::from_secs(5), "held {held:?}");
        // A longer hold than the session bound (this server's longest wait, shorter than
        // `COLLECT_WAIT`) is answered at the bound.
        let started = Instant::now();
        assert_eq!(collect_held(ha, 3_600_000_000, FOREVER).unwrap(), ([0; 4], vec![]));
        let held = started.elapsed();
        let bound = Duration::from_micros(LONGEST.min(COLLECT_WAIT));
        assert!(held >= bound && held < bound + Duration::from_secs(5), "held {held:?}");
        // The session lives on.
        read(ha, 0, NOW_FID);
        assert_eq!(collect(ha), vec![(0, Got::Data(b"hello".to_vec()))]);
    });
    w.end();
}

#[test]
fn a_sends_pages_count_once_and_go_back_with_its_last_request() {
    let w = World::new();
    let f = fake();
    let (a, ha) = w.client(1011);
    let hello = || Got::Data(b"hello".to_vec());
    f.as_process(a, || {
        open_session(ha);
        // Two reads in one page: two requests and a page, each within its share.
        reads_in_one_transfer(ha, &[0, 1], NOW_FID);
    });
    until("the page and both reads are held", || w.seen.held(1011) == 1 + 3);
    f.as_process(a, || assert_eq!(collect(ha), vec![(0, hello()), (1, hello())]));
    until("the page went back with the last read", || w.seen.held(1011) == 1);
    // Two pages are more than the share of one: the read they brought is answered busy, and
    // nothing of it is held.
    f.as_process(a, || {
        reads_in_pages(ha, &[2], NOW_FID, 2);
        assert_eq!(collect(ha), vec![(2, busy())]);
    });
    until("nothing of the refused send is held", || w.seen.held(1011) == 1);
    w.end();
}
