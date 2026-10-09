//! The hub (`redoubt_client::aio`) against the 9P skeleton on the fake kernel: inline submission
//! that never waits long on a busy server, completions with no thread but the caller's or with a
//! waiter per connection, and every buffer back by value, once.

use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use redoubt_client::Name;
use redoubt_client::aio::{COLLECT_MARGIN_US, Conn, Done, Hub, MAX_WRITE, Outcome, SUBMIT_TIMEOUT_US};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, FOREVER, Handle, MAX_LEND_PAGES, PAGE_SIZE};
use redoubt_rt::client::{Connection, Lend};
use redoubt_rt::handle::{self, Endpoint};
use redoubt_rt::ipc::{Buffer, Caller, Event, Request};
use redoubt_rt::server::Limits;
use redoubt_rt::server::close_delivery;
use redoubt_rt::server::ninep::{
    COLLECT_WAIT, FileServer, FileStat, NineError, NineServer, Qid, Read, mode, refuse_malformed,
};
use redoubt_rt::wire::ninep::{Body, Names};

/// A root holding `now`, read at once, and `gate`, whose reads wait until the gate opens.
struct Files {
    open: Arc<AtomicBool>,
}

const ROOT: u8 = 0;
const NOW: u8 = 1;
const GATE: u8 = 2;

fn qid(node: u8) -> Qid { Qid { kind: if node == ROOT { 0x80 } else { 0 }, version: 0, path: node.into() } }

impl FileServer for Files {
    type Node = u8;

    fn attach(&mut self, _: &Caller, _: &str) -> Result<(u8, Qid), NineError> { Ok((ROOT, qid(ROOT))) }

    fn labels(&self, _: &u8) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, _: &u8, name: &str) -> Result<(u8, Qid), NineError> {
        let node = match name {
            "now" => NOW,
            "gate" => GATE,
            _ => return Err(NineError::NOT_FOUND),
        };
        Ok((node, qid(node)))
    }

    fn open(&mut self, _: &Caller, node: &u8, _: u8) -> Result<Qid, NineError> { Ok(qid(*node)) }

    fn read(&mut self, _: &Caller, node: &u8, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        if *node == GATE && !self.open.load(Ordering::Acquire) {
            return Ok(Read::Wait);
        }
        let data: &[u8] = if *node == GATE { b"opened" } else { b"hello" };
        let data = data.get(offset as usize..).unwrap_or(&[]);
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

/// The transfers the server took: how many, and the most pages one had.
#[derive(Default)]
struct Transfers {
    count: AtomicUsize,
    widest: AtomicUsize,
}

/// The skeleton's loop (as `NineServer::run`), which stops receiving while `paused`.
fn serve(ep: Endpoint, open: Arc<AtomicBool>, paused: Arc<AtomicBool>, seen: Arc<Transfers>) -> u32 {
    let limits = Limits { buckets: 2, in_flight: 8, files: 16, state: 0, requests: 128, pages: 4 };
    let mut nine = NineServer::new(Files { open }, limits, 7).unwrap();
    let own = |_: &mut NineServer<Files>, r: Request| refuse_malformed(r).map(|()| None);
    loop {
        while paused.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
        let now = handle::time_now().unwrap();
        nine.expire(now);
        // Short, so a pause is seen soon.
        let timeout = nine.next_deadline().map_or(5_000, |d| d.saturating_sub(now).clamp(1, 5_000));
        match ep.receive(timeout, MAX_LEND_PAGES) {
            Ok(Event::Call(request)) => {
                let _ = nine.serve_with(request, own);
            }
            Ok(Event::Send(delivery)) => {
                if let Some(pages) = &delivery.transfer {
                    seen.count.fetch_add(1, Ordering::AcqRel);
                    seen.widest.fetch_max(pages.npages(), Ordering::AcqRel);
                }
                if let Some(other) = nine.deliver(delivery, now) {
                    close_delivery(&other);
                }
            }
            Ok(Event::Abandoned(id)) => {
                nine.abandoned(id);
            }
            Ok(_) | Err(Error::Timeout) => {}
            Err(_) => return 0,
        }
        // A gate opened lets waiting reads go.
        nine.wake(handle::time_now().unwrap());
    }
}

/// Fids every connection opens by one-call 9P first.
const NOW_FID: u32 = 1;
const GATE_FID: u32 = 2;

/// A server, and a client process (account 0, so its badge is a bucket of its own).
struct World {
    server: usize,
    receive: Handle,
    thread: std::thread::JoinHandle<u32>,
    client: usize,
    open: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    transfers: Arc<Transfers>,
}

impl World {
    fn new() -> World {
        let f = fake();
        let server = f.process(0, &[]);
        let receive = f.endpoint(server);
        let (open, paused) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let transfers = Arc::new(Transfers::default());
        let (o, p, t) = (open.clone(), paused.clone(), transfers.clone());
        let thread = f.run(server, move || serve(Endpoint::from_handle(receive), o, p, t));
        World { server, receive, thread, client: f.process(0, &[]), open, paused, transfers }
    }

    /// A connection on its own badge, its files open by one-call 9P.
    fn connection(&self, badge: u64) -> Handle {
        let ep = fake().grant(self.server, self.receive, self.client, badge);
        fake().as_process(self.client, || {
            let c = Connection::new(Endpoint::from_handle(ep));
            let mut lend = Lend::new(1).unwrap();
            c.attach(&mut lend, 0, "").unwrap();
            for (fid, name) in [(NOW_FID, "now"), (GATE_FID, "gate")] {
                c.walk(&mut lend, 0, fid, name).unwrap();
                c.open(&mut lend, fid, mode::OREAD).unwrap();
            }
        });
        ep
    }

    fn end(self) {
        fake().destroy(self.server, self.receive);
        assert_eq!(self.thread.join().unwrap(), 0);
    }
}

/// A one-page buffer, and where its pages are, to know it again.
fn buffer() -> (Buffer, usize) {
    let b = Buffer::new(1).unwrap();
    let at = b.as_ptr() as usize;
    (b, at)
}

/// Waits in the completion call until a completion comes, and takes it.
fn next(hub: &mut Hub, conn: Conn) -> Done {
    loop {
        if let Some(done) = hub.completed() {
            return done;
        }
        hub.wait(conn, 1_000_000).unwrap();
    }
}

fn data(done: &Done) -> &[u8] {
    let Outcome::Read(n) = done.outcome else { panic!("not a read: {done:?}") };
    &done.buffer.as_ref().unwrap()[..n]
}

#[test]
fn an_inline_submit_to_a_busy_server_returns_and_goes_at_the_next_poll() {
    let w = World::new();
    let ep = w.connection(1);
    let paused = w.paused.clone();
    fake().as_process(w.client, || {
        let mut hub = Hub::new();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        // The server stops taking anything: the submit returns once its send has timed out.
        paused.store(true, Ordering::Release);
        std::thread::sleep(Duration::from_millis(20));
        let (b, at) = buffer();
        let started = Instant::now();
        let tag = hub.read(conn, NOW_FID, 0, b).unwrap();
        let took = started.elapsed();
        assert!(took >= Duration::from_micros(SUBMIT_TIMEOUT_US), "returned before its timeout");
        assert!(took < Duration::from_millis(500), "the submit waited on the server: {took:?}");
        assert_eq!(hub.queued(), 1, "the request is queued, not lost");
        // The server takes again: the next poll sends it, and it is answered.
        paused.store(false, Ordering::Release);
        let started = Instant::now();
        while hub.queued() != 0 {
            assert!(started.elapsed() < Duration::from_secs(20), "never sent");
            hub.poll();
        }
        let done = next(&mut hub, conn);
        assert_eq!((done.tag, data(&done)), (tag, &b"hello"[..]));
        assert_eq!(done.buffer.as_ref().unwrap().as_ptr() as usize, at);
    });
    w.end();
}

#[test]
fn an_rerror_through_the_hub_keeps_its_name() {
    let w = World::new();
    let ep = w.connection(9);
    fake().as_process(w.client, || {
        let mut hub = Hub::new();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        // The server answers `file does not exist`, and an open of an open fid `fid is open`.
        let wnames = Names::new(&["missing"]).unwrap();
        let walk = hub.submit(conn, Body::Twalk { fid: 0, newfid: 40, wnames }, None).unwrap();
        let open = hub.submit(conn, Body::Topen { fid: NOW_FID, mode: mode::OREAD }, None).unwrap();
        let mut got = [next(&mut hub, conn), next(&mut hub, conn)];
        got.sort_by_key(|d| d.tag);
        assert_eq!((got[0].tag, &got[0].outcome), (walk, &Outcome::Rerror(Name::NotFound)));
        assert_eq!((got[1].tag, &got[1].outcome), (open, &Outcome::Rerror(Name::Protocol)));
    });
    w.end();
}

#[test]
fn one_connection_needs_no_waiter_thread() {
    let w = World::new();
    let ep = w.connection(2);
    let transfers = w.transfers.clone();
    fake().as_process(w.client, || {
        let mut hub = Hub::new();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        // 64 reads outstanding, sent together in one transfer of a page (on either width), and
        // answered to the caller's own completion calls.
        let tags: Vec<u16> =
            hub.batch(|hub| (0..64).map(|_| hub.read(conn, NOW_FID, 0, buffer().0).unwrap()).collect());
        let mut seen = Vec::new();
        while seen.len() < tags.len() {
            let done = next(&mut hub, conn);
            assert_eq!(data(&done), b"hello");
            seen.push(done.tag);
        }
        seen.sort_unstable();
        assert_eq!(seen, tags);
        let took = (transfers.count.load(Ordering::Acquire), transfers.widest.load(Ordering::Acquire));
        assert_eq!(took, (1, 1), "64 reads in one transfer of a page");
        // With nothing outstanding, a wait ends when its hold does, and the session lives on.
        let started = Instant::now();
        hub.wait(conn, 50_000).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(50));
        assert!(hub.completed().is_none());
        hub.read(conn, NOW_FID, 0, buffer().0).unwrap();
        assert_eq!(data(&next(&mut hub, conn)), b"hello");
    });
    w.end();
}

#[test]
fn buffers_come_back_to_their_submitter_by_value_in_any_order() {
    let w = World::new();
    let ep = w.connection(3);
    let open = w.open.clone();
    fake().as_process(w.client, || {
        let mut hub = Hub::new();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        // In tag order.
        let sent: Vec<(u16, usize)> = (0..3)
            .map(|_| {
                let (b, at) = buffer();
                (hub.read(conn, NOW_FID, 0, b).unwrap(), at)
            })
            .collect();
        for (tag, at) in &sent {
            let done = next(&mut hub, conn);
            assert_eq!((done.tag, done.buffer.as_ref().unwrap().as_ptr() as usize), (*tag, *at));
        }
        // Out of it: the first waits at the gate while the second is answered.
        let (first, first_at) = buffer();
        let (second, second_at) = buffer();
        let gate = hub.read(conn, GATE_FID, 0, first).unwrap();
        let now = hub.read(conn, NOW_FID, 0, second).unwrap();
        let done = next(&mut hub, conn);
        assert_eq!((done.tag, done.buffer.as_ref().unwrap().as_ptr() as usize), (now, second_at));
        open.store(true, Ordering::Release);
        let done = next(&mut hub, conn);
        assert_eq!((done.tag, done.buffer.as_ref().unwrap().as_ptr() as usize), (gate, first_at));
        assert_eq!(data(&done), b"opened");
    });
    w.end();
}

#[test]
fn a_flushed_requests_buffer_is_returned_exactly_once() {
    let w = World::new();
    let ep = w.connection(4);
    let paused = w.paused.clone();
    fake().as_process(w.client, || {
        let mut hub = Hub::new();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        // Sent and waiting at the server: the Rflush brings its buffer back, and nothing else does.
        let (b, at) = buffer();
        let read = hub.read(conn, GATE_FID, 0, b).unwrap();
        let flush = hub.submit(conn, Body::Tflush { oldtag: read }, None).unwrap();
        let mut done = vec![next(&mut hub, conn), next(&mut hub, conn)];
        hub.wait(conn, 50_000).unwrap();
        done.extend(std::iter::from_fn(|| hub.completed()));
        let back: Vec<_> =
            done.iter().filter(|d| d.buffer.as_ref().is_some_and(|b| b.as_ptr() as usize == at)).collect();
        assert_eq!(back.len(), 1, "the buffer came back {} times", back.len());
        assert_eq!((back[0].tag, &back[0].outcome), (read, &Outcome::Flushed));
        assert!(done.iter().any(|d| d.tag == flush && d.outcome == Outcome::Flushed));
        assert_eq!(done.len(), 2);
        // Still queued when flushed: neither goes to the server, and the buffer is back at once.
        paused.store(true, Ordering::Release);
        std::thread::sleep(Duration::from_millis(20));
        let (b, at) = buffer();
        let read = hub.read(conn, NOW_FID, 0, b).unwrap();
        assert_eq!(hub.queued(), 1);
        hub.submit(conn, Body::Tflush { oldtag: read }, None).unwrap();
        assert_eq!(hub.queued(), 0);
        let done = hub.completed().unwrap();
        assert_eq!(
            (done.tag, done.outcome, done.buffer.unwrap().as_ptr() as usize),
            (read, Outcome::Flushed, at)
        );
        assert_eq!(hub.completed().unwrap().outcome, Outcome::Flushed);
        paused.store(false, Ordering::Release);
        hub.wait(conn, 50_000).unwrap();
        assert!(hub.completed().is_none(), "something of the flushed read came back again");
    });
    w.end();
}

/// A flush goes out while its target's answer is already on its way back: the answer frees the
/// target's slot, but its tag stays taken until the `Rflush`, so a new request never gets it, and
/// the `Rflush` flushes nothing of the new one.
#[test]
fn a_tag_a_flush_names_is_not_reused_before_its_rflush() {
    let w = World::new();
    let ep = w.connection(9);
    let open = w.open.clone();
    fake().as_process(w.client, || {
        let mut hub = Hub::new();
        let receive = Endpoint::create().unwrap();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        hub.spawn_waiter(conn, &receive, NonZeroU64::new(100).unwrap()).unwrap();
        let wake = || match receive.receive(10_000_000, MAX_LEND_PAGES).unwrap() {
            Event::Send(delivery) => delivery,
            other => panic!("{other:?}"),
        };
        let read = hub.read(conn, NOW_FID, 0, buffer().0).unwrap();
        sent(&mut hub);
        // Its answer is in the waiter's wake-up, not yet in the hub, when the flush goes.
        let answer = wake();
        let flush = hub.submit(conn, Body::Tflush { oldtag: read }, None).unwrap();
        sent(&mut hub);
        assert!(hub.deliver(answer).is_none());
        let done = hub.completed().unwrap();
        assert_eq!((done.tag, &done.outcome), (read, &Outcome::Read(5)));
        // A new request while the Rflush is still to come: it does not get the read's tag.
        let gate = hub.read(conn, GATE_FID, 0, buffer().0).unwrap();
        sent(&mut hub);
        assert!(gate != read && gate != flush, "tag {gate} reused");
        let mut done = Vec::new();
        while !done.iter().any(|d: &Done| d.tag == flush) {
            assert!(hub.deliver(wake()).is_none());
            done.extend(std::iter::from_fn(|| hub.completed()));
        }
        assert_eq!(
            done.iter().map(|d| (d.tag, d.outcome.clone())).collect::<Vec<_>>(),
            [(flush, Outcome::Flushed)]
        );
        // The new request is still outstanding, and answered when the gate opens.
        open.store(true, Ordering::Release);
        assert!(hub.deliver(wake()).is_none());
        let done = hub.completed().unwrap();
        assert_eq!((done.tag, data(&done)), (gate, &b"opened"[..]));
    });
    w.end();
}

/// A tag stays its request's until the caller takes the completion, not only until the hub reads
/// the answer: a server answers out of order (a waiting read after a later one), so an answer read
/// but not taken can hold the lowest tag, and a request sent meanwhile would share it, and the
/// caller would give one request's answer to the other.
#[test]
fn a_tag_is_not_reused_before_its_completion_is_taken() {
    let w = World::new();
    let ep = w.connection(9);
    let open = w.open.clone();
    fake().as_process(w.client, || {
        let mut hub = Hub::new();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        // The gate's read waits; the later read is answered first, and taken.
        let gate = hub.read(conn, GATE_FID, 0, buffer().0).unwrap();
        let now = hub.read(conn, NOW_FID, 0, buffer().0).unwrap();
        sent(&mut hub);
        let done = next(&mut hub, conn);
        assert_eq!((done.tag, data(&done)), (now, &b"hello"[..]));
        // The gate's answer is read, and not taken, when the next request goes.
        open.store(true, Ordering::Release);
        hub.wait(conn, 1_000_000).unwrap();
        let again = hub.read(conn, NOW_FID, 0, buffer().0).unwrap();
        assert_ne!(again, gate, "the gate's tag reused before its completion was taken");
        let done = next(&mut hub, conn);
        assert_eq!((done.tag, data(&done)), (gate, &b"opened"[..]));
        let done = next(&mut hub, conn);
        assert_eq!((done.tag, data(&done)), (again, &b"hello"[..]));
    });
    w.end();
}

/// Requests batched past a page go a page at a time, so no send needs more of a server's `Pages`
/// than one.
#[test]
fn a_batch_goes_a_page_at_a_time() {
    let w = World::new();
    let ep = w.connection(10);
    let transfers = w.transfers.clone();
    fake().as_process(w.client, || {
        let mut hub = Hub::new();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        // 200 reads of 23 bytes: 4 600, past one page.
        let tags: Vec<u16> =
            hub.batch(|hub| (0..200).map(|_| hub.read(conn, NOW_FID, 0, buffer().0).unwrap()).collect());
        // Each is answered, or refused past the share: none is lost.
        let mut seen = Vec::new();
        while seen.len() < tags.len() {
            let done = next(&mut hub, conn);
            assert!(matches!(done.outcome, Outcome::Read(5) | Outcome::Busy), "{done:?}");
            seen.push(done.tag);
        }
        seen.sort_unstable();
        assert_eq!(seen, tags);
    });
    // What the server took, whenever each send was taken: two transfers of one page.
    assert_eq!(transfers.count.load(Ordering::Acquire), 2, "200 reads in two transfers");
    assert_eq!(transfers.widest.load(Ordering::Acquire), 1, "a transfer of more than a page");
    w.end();
}

/// A write goes in one page, its `Twrite` and all: `MAX_WRITE` bytes are one transfer of a page and
/// taken whole, one byte more is the caller's to split and sends nothing.
#[test]
fn a_write_is_at_most_one_page() {
    let w = World::new();
    let ep = w.connection(11);
    let (client, transfers) = (w.client, w.transfers.clone());
    fake().as_process(client, || {
        const WRITE_FID: u32 = 3;
        let c = Connection::new(Endpoint::from_handle(ep));
        let mut lend = Lend::new(1).unwrap();
        c.walk(&mut lend, 0, WRITE_FID, "now").unwrap();
        c.open(&mut lend, WRITE_FID, mode::OWRITE).unwrap();
        let mut hub = Hub::new();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        let tag = hub.write(conn, WRITE_FID, 0, Buffer::new(2).unwrap(), MAX_WRITE).unwrap();
        let done = next(&mut hub, conn);
        assert_eq!((done.tag, done.outcome), (tag, Outcome::Wrote(MAX_WRITE as u32)));
        let took = (transfers.count.load(Ordering::Acquire), transfers.widest.load(Ordering::Acquire));
        assert_eq!(took, (1, 1), "one transfer of a page");
        let sends = || fake().calls(client).iter().filter(|c| **c == "send").count();
        let before = sends();
        let refused = hub.write(conn, WRITE_FID, 0, Buffer::new(2).unwrap(), MAX_WRITE + 1);
        assert_eq!(refused.unwrap_err(), redoubt_client::Error::Wire(redoubt_rt::wire::Error::TooLarge));
        assert_eq!((sends() - before, hub.queued()), (0, 0), "nothing of it went");
    });
    w.end();
}

#[test]
fn two_connections_have_a_waiter_each_and_the_caller_idles_in_receive() {
    let w = World::new();
    let (a, b) = (w.connection(5), w.connection(6));
    let client = w.client;
    fake().as_process(client, || {
        let mut hub = Hub::new();
        let receive = Endpoint::create().unwrap();
        let conns =
            [hub.connect(Endpoint::from_handle(a)).unwrap(), hub.connect(Endpoint::from_handle(b)).unwrap()];
        for (i, conn) in conns.iter().enumerate() {
            hub.spawn_waiter(*conn, &receive, NonZeroU64::new(100 + i as u64).unwrap()).unwrap();
        }
        for conn in conns {
            for _ in 0..8 {
                hub.read(conn, NOW_FID, 0, buffer().0).unwrap();
            }
        }
        sent(&mut hub);
        let mut answered = [0; 2];
        while answered != [8, 8] {
            match receive.receive(10_000_000, MAX_LEND_PAGES).unwrap() {
                Event::Send(delivery) => {
                    assert!(hub.deliver(delivery).is_none(), "a wake-up the hub did not take")
                }
                other => panic!("{other:?}"),
            }
            while let Some(done) = hub.completed() {
                assert_eq!(data(&done), b"hello");
                answered[usize::from(done.conn != conns[0])] += 1;
            }
        }
        // A wake-up from anyone but a waiter is handed back.
        let stranger = receive.mint(NonZeroU64::new(7).unwrap(), None).unwrap().handle();
        let words = [redoubt_client::aio::WAKE, 0, 0, 0];
        let sender = fake().run(client, move || {
            u32::from(Endpoint::from_handle(stranger).send(&words, &[], None, FOREVER).is_ok())
        });
        let Event::Send(delivery) = receive.receive(FOREVER, 0).unwrap() else { panic!() };
        assert!(hub.deliver(delivery).is_some());
        assert_eq!(sender.join().unwrap(), 1);
    });
    w.end();
}

/// A caller away from its endpoint for longer than the session bound keeps its session: the
/// waiter holds the answer it could not hand over, in the answer's own page, and keeps calling.
#[test]
fn a_caller_busy_past_the_session_bound_keeps_its_session() {
    let w = World::new();
    let ep = w.connection(12);
    fake().as_process(w.client, || {
        let mut hub = Hub::new();
        let receive = Endpoint::create().unwrap();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        hub.spawn_waiter(conn, &receive, NonZeroU64::new(100).unwrap()).unwrap();
        let take = |hub: &mut Hub| loop {
            if let Some(done) = hub.completed() {
                return done;
            }
            let Event::Send(delivery) = receive.receive(10_000_000, MAX_LEND_PAGES).unwrap() else {
                panic!()
            };
            assert!(hub.deliver(delivery).is_none());
        };
        // Busy elsewhere, past the session bound, with an answer ready.
        let first = hub.read(conn, NOW_FID, 0, buffer().0).unwrap();
        sent(&mut hub);
        std::thread::sleep(Duration::from_micros(COLLECT_WAIT + 2_000_000));
        let Event::Send(delivery) = receive.receive(10_000_000, MAX_LEND_PAGES).unwrap() else { panic!() };
        let held = delivery.transfer.as_ref().unwrap().len();
        assert_eq!(held, PAGE_SIZE, "the held answer kept the whole completion buffer");
        assert!(hub.deliver(delivery).is_none());
        let done = take(&mut hub);
        assert_eq!((done.tag, data(&done)), (first, &b"hello"[..]));
        // The session lives on: a new request is answered, not ended.
        let second = hub.read(conn, NOW_FID, 0, buffer().0).unwrap();
        sent(&mut hub);
        let done = take(&mut hub);
        assert_eq!((done.tag, data(&done)), (second, &b"hello"[..]));
    });
    w.end();
}

#[test]
fn a_server_that_breaks_its_hold_loses_the_session_at_the_margin() {
    let w = World::new();
    let ep = w.connection(8);
    let f = fake();
    let (server, paused) = (w.server, w.paused.clone());
    // Set once the read is taken: until then the server may hold the session's opening call, which
    // `until_parked` would take for the completion call and pause the server before the read.
    let taken = Arc::new(AtomicBool::new(false));
    let read_taken = taken.clone();
    let client = f.run(w.client, move || {
        let mut hub = Hub::new();
        let conn = hub.connect(Endpoint::from_handle(ep)).unwrap();
        let (b, at) = buffer();
        let tag = hub.read(conn, GATE_FID, 0, b).unwrap();
        // One still queued would make the wait hold at most `RETRY_US`, not the 50 ms this test
        // measures from.
        sent(&mut hub);
        read_taken.store(true, Ordering::Release);
        // The server takes the call and stops answering: the call times out a margin past its hold.
        let started = Instant::now();
        hub.wait(conn, 50_000).unwrap();
        let waited = started.elapsed();
        assert!(waited >= Duration::from_micros(50_000 + COLLECT_MARGIN_US), "gave up at {waited:?}");
        // The session is lost: the read comes back ended, with its buffer.
        let done = hub.completed().unwrap();
        assert_eq!(
            (done.tag, done.outcome, done.buffer.unwrap().as_ptr() as usize),
            (tag, Outcome::Ended, at)
        );
        assert_eq!(hub.read(conn, NOW_FID, 0, buffer().0).unwrap_err(), redoubt_client::Error::Disconnected);
        1
    });
    let started = Instant::now();
    while !taken.load(Ordering::Acquire) {
        assert!(started.elapsed() < Duration::from_secs(20), "the read was never sent");
        std::thread::sleep(Duration::from_millis(1));
    }
    until_parked(server);
    paused.store(true, Ordering::Release);
    assert_eq!(client.join().unwrap(), 1);
    // The server, answering again, is told of the abandoned call and ends the session: a new
    // completion call opens a fresh one.
    paused.store(false, Ordering::Release);
    f.as_process(w.client, || {
        let started = Instant::now();
        let mut hub = Hub::new();
        while hub.connect(Endpoint::from_handle(ep)).is_err() {
            assert!(started.elapsed() < Duration::from_secs(20), "the old session never ended");
        }
    });
    w.end();
}

/// Polls until the server has taken everything queued: a server on a busy host can miss a
/// submit's 1 ms, and a test that then idles without entering the hub, or times a wait whose hold
/// assumed nothing was queued, would wait on a request never sent.
fn sent(hub: &mut Hub) {
    let started = Instant::now();
    while hub.queued() != 0 {
        assert!(started.elapsed() < Duration::from_secs(20), "never sent");
        hub.poll();
    }
}

/// Waits until `server` holds the completion call.
fn until_parked(server: usize) {
    let started = Instant::now();
    while fake().open_calls(server) != 1 {
        assert!(started.elapsed() < Duration::from_secs(20), "the completion call was never parked");
        std::thread::sleep(Duration::from_millis(1));
    }
}
