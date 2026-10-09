//! `consrelay`, the whole program, as a fake process: its four threads run, it sends the steward
//! its hello, serves a VM's `/dev/cons`, and forwards it to the console of whichever channel the
//! steward attaches, here a stand-in for `sshd`'s that records what is written to it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use redoubt_consrelay::LIMITS;
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::client::{Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Delivery, Event, Request};
use redoubt_rt::server::ninep::{
    Around, FileServer, FileStat, NineError, NineServer, Qid, Read, Write, mode, refuse, refuse_malformed,
};
use redoubt_rt::server::parked::{NotParked, Parked};
use redoubt_rt::server::{Limits, MALFORMED, consol as consol_server};
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::proto::consol;
use redoubt_rt::wire::proto::consrelay::{Attach, Detach, Message, Reply};

#[path = "../src/bin/consrelay.rs"]
mod relay;

/// The context's account and labels: the relay's and the VM's.
const ACCOUNT: u64 = 7;
const LABELS: &[u64] = &[42];
const WAIT: Duration = Duration::from_secs(5);

/// A channel's console, as `sshd` serves one: what is written to it is recorded, what is put in
/// `input` is read from it, and `consol` is answered with `size`, its count of changes in `resized`.
/// While `stalled`, a write waits, as on a channel whose client has stopped reading.
#[derive(Clone)]
struct Pipe {
    out: Arc<Mutex<Vec<u8>>>,
    input: Arc<Mutex<VecDeque<u8>>>,
    size: Arc<Mutex<((u16, u16), u64)>>,
    stalled: Arc<AtomicBool>,
    /// The client's input has ended: a read with no input finds the end of the file.
    ended: Arc<AtomicBool>,
}

impl Default for Pipe {
    fn default() -> Pipe {
        Pipe {
            out: Arc::default(),
            input: Arc::default(),
            size: Arc::new(Mutex::new(((80, 24), 0))),
            stalled: Arc::default(),
            ended: Arc::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Node;

fn qid() -> Qid { Qid { kind: 0, version: 0, path: 0 } }

impl FileServer for Pipe {
    type Node = Node;

    fn attach(&mut self, _: &Caller, _: &str) -> Result<(Node, Qid), NineError> { Ok((Node, qid())) }

    fn labels(&self, _: &Node) -> &[u64] { LABELS }

    fn walk(&mut self, _: &Caller, _: &Node, _: &str) -> Result<(Node, Qid), NineError> {
        Err(NineError::NOT_DIR)
    }

    fn open(&mut self, _: &Caller, _: &Node, _: u8) -> Result<Qid, NineError> { Ok(qid()) }

    fn read(&mut self, _: &Caller, _: &Node, _: u64, out: &mut [u8]) -> Result<Read, NineError> {
        let mut input = self.input.lock().unwrap();
        if input.is_empty() {
            return Ok(if self.ended.load(Ordering::SeqCst) { Read::Done(0) } else { Read::Wait });
        }
        let n = out.len().min(input.len());
        for (slot, b) in out.iter_mut().zip(input.drain(..n)) {
            *slot = b;
        }
        Ok(Read::Done(n))
    }

    fn write(&mut self, _: &Caller, _: &Node, _: u64, data: &[u8]) -> Result<usize, NineError> {
        self.out.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }

    fn write_or_wait(
        &mut self,
        c: &Caller,
        node: &Node,
        offset: u64,
        data: &[u8],
    ) -> Result<Write, NineError> {
        if self.stalled.load(Ordering::SeqCst) {
            return Ok(Write::Wait);
        }
        self.write(c, node, offset, data).map(Write::Done)
    }

    fn stat(&mut self, _: &Caller, _: &Node) -> Result<FileStat, NineError> {
        Ok(FileStat { qid: qid(), mode: 0o666, mtime: 0, length: 0, name: String::from("cons") })
    }

    fn dir_entry(&mut self, _: &Caller, _: &Node, _: u64) -> Result<Option<(Node, FileStat)>, NineError> {
        Ok(None)
    }
}

/// The stand-in's loop: reads with no input park, a `resize` parks until the size changes, and a
/// send wakes them.
struct Readers(Parked<Option<u64>>);

impl Readers {
    fn serve(&mut self, server: &mut NineServer<Pipe>, request: Request, now: u64) {
        let (size, resized) = *server.fs.size.lock().unwrap();
        let held = server.serve_parking(request, |_, r| match consol_server::asks(&r.words) {
            true => consol_server::serve(size, r),
            false => refuse_malformed(r).map(|()| None),
        });
        let Ok(Some(request)) = held else { return };
        let waiting = consol_server::asks(&request.words).then_some(resized);
        let charge = server.charge_of(&request.caller);
        if let Err(NotParked(request)) = self.0.park(server.admission_mut(), request, charge, waiting, now) {
            let _ = refuse(request, NineError::TOO_MANY);
        }
    }
}

impl Around<Pipe> for Readers {
    fn call(&mut self, server: &mut NineServer<Pipe>, request: Request, now: u64) {
        self.serve(server, request, now)
    }

    fn turn(&mut self, server: &mut NineServer<Pipe>, now: u64) {
        let (size, resized) = *server.fs.size.lock().unwrap();
        let due = |w: &Option<u64>| w.is_some_and(|from| from != resized);
        while let Some(Ok((request, _))) = self.0.resume_first(server.admission_mut(), due) {
            let _ = consol_server::reply_resize(request, size);
        }
        for _ in 0..self.0.len() {
            let Some(Ok((request, _))) = self.0.resume_first(server.admission_mut(), |w| w.is_none()) else {
                break;
            };
            self.serve(server, request, now);
        }
    }

    fn send(&mut self, _: &mut NineServer<Pipe>, delivery: Delivery, _: u64) {
        redoubt_rt::server::close_delivery(&delivery)
    }
}

/// A channel console: its process's endpoint, and a handle to it in the steward's table.
struct Console {
    pipe: Pipe,
    /// The steward's handle, which it hands the relay in `attach`.
    handle: Handle,
    /// The steward's badge for waking the console's parked reads.
    wake: Handle,
}

impl Console {
    fn start(steward: usize) -> Console {
        let f = fake();
        let pid = f.process(0, &[]);
        let receive = f.endpoint(pid);
        let handle = f.grant(pid, receive, steward, 77);
        let wake = f.grant(pid, receive, steward, 78);
        let pipe = Pipe::default();
        let served = pipe.clone();
        f.run(pid, move || {
            // Roomier than `sshd`'s console's limits, so that a write to a stalled channel waits
            // beside the reader's read and the sizer's `resize` instead of being refused: the
            // worse case for a detach.
            let limits = Limits { buckets: 4, in_flight: 16, ..LIMITS };
            let mut server = NineServer::new(served, limits, 0x5eed).expect("the limits fit");
            server.run_around(&Endpoint::from_handle(receive), Readers(Parked::new(FOREVER)))
        });
        Console { pipe, handle, wake }
    }

    /// The terminal is resized to `size`.
    fn resize(&self, steward: usize, size: (u16, u16)) {
        {
            let mut held = self.pipe.size.lock().unwrap();
            held.0 = size;
            held.1 += 1;
        }
        fake()
            .as_process(steward, || Endpoint::from_handle(self.wake).send(&[1, 0, 0, 0], &[], None, FOREVER))
            .expect("the console takes the wake");
    }

    /// Types `bytes` on the channel.
    fn type_in(&self, steward: usize, bytes: &[u8]) {
        self.pipe.input.lock().unwrap().extend(bytes);
        fake()
            .as_process(steward, || Endpoint::from_handle(self.wake).send(&[1, 0, 0, 0], &[], None, FOREVER))
            .expect("the console takes the wake");
    }

    /// The client's input ends, after `bytes`.
    fn end_input(&self, steward: usize, bytes: &[u8]) {
        self.pipe.input.lock().unwrap().extend(bytes);
        self.pipe.ended.store(true, Ordering::SeqCst);
        fake()
            .as_process(steward, || Endpoint::from_handle(self.wake).send(&[1, 0, 0, 0], &[], None, FOREVER))
            .expect("the console takes the wake");
    }

    /// The client stops reading (`true`) or reads again: a write waiting is tried again.
    fn stall(&self, steward: usize, stalled: bool) {
        self.pipe.stalled.store(stalled, Ordering::SeqCst);
        fake()
            .as_process(steward, || Endpoint::from_handle(self.wake).send(&[1, 0, 0, 0], &[], None, FOREVER))
            .expect("the console takes the wake");
    }

    /// Waits until what was written ends with `tail`; all that was written.
    fn wait_for(&self, tail: &[u8]) -> Vec<u8> {
        let start = Instant::now();
        loop {
            let out = self.pipe.out.lock().unwrap().clone();
            if out.ends_with(tail) {
                return out;
            }
            assert!(
                start.elapsed() < WAIT,
                "waiting for {:?}, have {:?}",
                String::from_utf8_lossy(tail),
                String::from_utf8_lossy(&out)
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// A relay started, and what the steward and the VM hold of it.
struct Box_ {
    steward: usize,
    vm: usize,
    /// The VM's console, in the VM's table.
    cons: Handle,
    /// The control badge, in the steward's table.
    control: Handle,
}

fn boot() -> Box_ {
    let f = fake();
    let steward = f.process(1, &[]);
    let relay = f.process(ACCOUNT, LABELS);
    let vm = f.process(ACCOUNT, LABELS);
    let hello = f.endpoint(steward);
    let badge = f.grant(steward, hello, relay, 5);
    let mut builder = StartupBuilder::new(badge.index());
    builder.handle(relay::HELLO, badge);
    let block = builder.finish().expect("the block");
    f.run(relay, move || relay::serve(&Startup::parse(&block).expect("the block parses")));
    let said = f.as_process(steward, || Endpoint::from_handle(hello).receive(WAIT.as_micros() as u64, 0));
    let Ok(Event::Send(d)) = said else { panic!("no hello: {said:?}") };
    assert!(matches!(Message::decode(&d.words, &[], 2), Ok(Message::Hello(_))));
    let handles: Vec<Handle> = d.handles.as_slice().iter().map(|h| h.expect("both handles came")).collect();
    let cons = f.copy(steward, handles[0], vm);
    Box_ { steward, vm, cons, control: handles[1] }
}

/// A `consrelay` call through `through` as `pid`: its reply, or its error code.
fn call(pid: usize, through: Handle, message: Message<'_>, handles: &[Handle]) -> Result<Reply, u32> {
    fake().as_process(pid, || {
        let mut lend = Lend::new(1).unwrap();
        let words = message.encode(lend.pages().unwrap()).unwrap();
        let outcome = lend.call(&Endpoint::from_handle(through), &words, handles, WAIT.as_micros() as u64);
        outcome.status.expect("the call is answered");
        let reply = outcome.reply.as_ref().expect("a reply").words;
        if reply == MALFORMED {
            return Err(1);
        }
        let opcode = match message {
            Message::Attach(_) => 24,
            _ => 25,
        };
        match Reply::decode(opcode, &reply, lend.bytes(), 0) {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(code)) => Err(code.code()),
            Err(_) => Err(u32::MAX),
        }
    })
}

/// The VM's console, opened as a VM's namespace opens it.
fn vm_console(b: &Box_) -> (Connection, Lend) {
    fake().as_process(b.vm, || {
        let nine = Connection::new(Endpoint::from_handle(b.cons));
        let mut lend = Lend::new(1).unwrap();
        nine.version(&mut lend).unwrap();
        nine.attach(&mut lend, 0, "").unwrap();
        nine.open(&mut lend, 0, mode::ORDWR).unwrap();
        (nine, lend)
    })
}

fn vm_write(b: &Box_, nine: &Connection, lend: &mut Lend, bytes: &[u8]) {
    let n = fake().as_process(b.vm, || nine.write(lend, 0, 0, bytes)).unwrap();
    assert_eq!(n, bytes.len());
}

#[test]
fn output_kept_while_detached_is_replayed_after_the_note_and_input_reaches_the_vm() {
    let b = boot();
    let (nine, mut lend) = vm_console(&b);
    // Nothing is attached: the write is kept, and does not wait.
    vm_write(&b, &nine, &mut lend, b"before\r\n");
    let one = Console::start(b.steward);
    let attached = call(b.steward, b.control, Message::Attach(Attach { note: "[hi]\r\n" }), &[one.handle]);
    assert!(attached.is_ok(), "{attached:?}");
    assert_eq!(one.wait_for(b"before\r\n"), b"[hi]\r\nbefore\r\n");
    vm_write(&b, &nine, &mut lend, b"live");
    one.wait_for(b"live");
    one.type_in(b.steward, b"ls\n");
    let mut out = [0u8; 16];
    let n = fake().as_process(b.vm, || nine.read(&mut lend, 0, 0, &mut out)).unwrap();
    assert_eq!(&out[..n], b"ls\n");

    // The steward lets the channel go with a note: the call returns once it is written.
    let detached = call(b.steward, b.control, Message::Detach(Detach { note: "[bye]\r\n" }), &[]);
    assert!(detached.is_ok(), "{detached:?}");
    assert!(one.pipe.out.lock().unwrap().ends_with(b"[bye]\r\n"));
    vm_write(&b, &nine, &mut lend, b"while away");
    std::thread::sleep(Duration::from_millis(50));
    assert!(!one.pipe.out.lock().unwrap().ends_with(b"while away"), "a channel let go gets nothing more");

    // The next channel gets what was written while none was attached.
    let two = Console::start(b.steward);
    call(b.steward, b.control, Message::Attach(Attach { note: "[back]\r\n" }), &[two.handle]).unwrap();
    assert_eq!(two.wait_for(b"while away"), b"[back]\r\nwhile away");
    fake().as_process(b.vm, || drop((nine, lend)));
}

#[test]
fn the_vm_cannot_attach_or_detach() {
    let b = boot();
    let console = Console::start(b.steward);
    let theirs = fake().copy(b.steward, console.handle, b.vm);
    // The VM's own badge: its control call is no 9P and no call of its connection.
    let attach = call(b.vm, b.cons, Message::Attach(Attach { note: "[mine]\r\n" }), &[theirs]);
    assert_eq!(attach, Err(1));
    assert_eq!(call(b.vm, b.cons, Message::Detach(Detach { note: "" }), &[]), Err(1));
    let (nine, mut lend) = vm_console(&b);
    vm_write(&b, &nine, &mut lend, b"kept");
    std::thread::sleep(Duration::from_millis(50));
    assert!(console.pipe.out.lock().unwrap().is_empty(), "nothing reached the channel the VM named");
    // The steward's attach still works, and replays what was kept.
    call(b.steward, b.control, Message::Attach(Attach { note: "" }), &[console.handle]).unwrap();
    assert_eq!(console.wait_for(b"kept"), b"kept");
    fake().as_process(b.vm, || drop((nine, lend)));
}

#[test]
fn a_control_call_without_its_console_is_malformed() {
    let b = boot();
    assert_eq!(call(b.steward, b.control, Message::Attach(Attach { note: "" }), &[]), Err(1));
}

/// A flood while detached, in console-sized writes as the VM's shell makes them: none waits, the
/// newest 64 KiB is kept, and the next channel gets the note, the drop count and the kept lines.
#[test]
fn a_flood_while_detached_is_counted_and_the_rest_replayed() {
    let b = boot();
    let (nine, mut lend) = vm_console(&b);
    let one = Console::start(b.steward);
    call(b.steward, b.control, Message::Attach(Attach { note: "" }), &[one.handle]).unwrap();
    vm_write(&b, &nine, &mut lend, b"prompt> ");
    one.wait_for(b"prompt> ");
    call(b.steward, b.control, Message::Detach(Detach { note: "" }), &[]).unwrap();
    let line = [b'z'; 999].iter().chain(b"\n").copied().collect::<Vec<u8>>();
    for _ in 0..70 {
        vm_write(&b, &nine, &mut lend, &line);
    }
    vm_write(&b, &nine, &mut lend, b"END\n");
    let two = Console::start(b.steward);
    call(b.steward, b.control, Message::Attach(Attach { note: "[back]\r\n" }), &[two.handle]).unwrap();
    let got = two.wait_for(b"END\n");
    let text = String::from_utf8_lossy(&got[..64]).into_owned();
    assert!(text.starts_with("[back]\r\n["), "{text:?}");
    assert!(text.contains("bytes of output dropped while detached]\r\n"), "{text:?}");
    fake().as_process(b.vm, || drop((nine, lend)));
}

/// A line typed just before the terminal closes reaches the VM's waiting read though the detach
/// comes at once, and what the VM then writes is kept, not held for the next channel.
#[test]
fn the_last_line_typed_reaches_the_vm_after_the_detach() {
    let b = boot();
    let (nine, mut lend) = vm_console(&b);
    let one = Console::start(b.steward);
    call(b.steward, b.control, Message::Attach(Attach { note: "" }), &[one.handle]).unwrap();
    vm_write(&b, &nine, &mut lend, b"prompt> ");
    one.wait_for(b"prompt> ");
    let vm = b.vm;
    let reading = std::thread::spawn(move || {
        let mut out = [0u8; 16];
        let n = fake().as_process(vm, || nine.read(&mut lend, 0, 0, &mut out)).unwrap();
        (out[..n].to_vec(), nine, lend)
    });
    std::thread::sleep(Duration::from_millis(50));
    one.type_in(b.steward, b"exit\n");
    call(b.steward, b.control, Message::Detach(Detach { note: "" }), &[]).unwrap();
    let (got, nine, mut lend) = reading.join().unwrap();
    assert_eq!(got, b"exit\n");
    vm_write(&b, &nine, &mut lend, b"after\n");
    let two = Console::start(b.steward);
    call(b.steward, b.control, Message::Attach(Attach { note: "[back]\r\n" }), &[two.handle]).unwrap();
    assert_eq!(two.wait_for(b"after\n"), b"[back]\r\nafter\n");
    fake().as_process(b.vm, || drop((nine, lend)));
}

/// A takeover from a channel whose client has stopped reading: the writer is in the middle of a
/// write there past the note's bound, so the detach is answered at the bound, well within the
/// steward's 1 s, without the note, and the channel is let go. Once the old write moves (here
/// the client reads again; on the box `sshd` ends the channel), the note follows it there, and
/// the new channel gets what the VM writes.
#[test]
fn a_detach_from_a_stalled_channel_is_answered_at_its_bound() {
    let b = boot();
    let (nine, mut lend) = vm_console(&b);
    let one = Console::start(b.steward);
    call(b.steward, b.control, Message::Attach(Attach { note: "" }), &[one.handle]).unwrap();
    vm_write(&b, &nine, &mut lend, b"prompt> ");
    one.wait_for(b"prompt> ");
    one.stall(b.steward, true);
    vm_write(&b, &nine, &mut lend, b"stuck");
    // The writer has taken "stuck" and waits in its write.
    std::thread::sleep(Duration::from_millis(50));
    let start = Instant::now();
    let note = "[taken over]\r\n";
    call(b.steward, b.control, Message::Detach(Detach { note }), &[]).unwrap();
    let waited = start.elapsed();
    let bound = Duration::from_micros(relay::NOTE_WAIT_US);
    assert!(waited >= bound && waited < Duration::from_millis(900), "the detach waited {waited:?}");
    assert!(!one.pipe.out.lock().unwrap().ends_with(note.as_bytes()), "no note while stalled");
    let two = Console::start(b.steward);
    call(b.steward, b.control, Message::Attach(Attach { note: "[back]\r\n" }), &[two.handle]).unwrap();
    one.stall(b.steward, false);
    assert_eq!(one.wait_for(note.as_bytes()), b"prompt> stuck[taken over]\r\n");
    vm_write(&b, &nine, &mut lend, b"new");
    assert_eq!(two.wait_for(b"new"), b"[back]\r\nnew");
    fake().as_process(b.vm, || drop((nine, lend)));
}

/// A takeover while the VM prints: the writer is in the middle of a chunk on a channel that
/// reads, so the detach waits for it to finish and for the note after it, and returns with the
/// old channel told.
#[test]
fn a_busy_writer_s_channel_gets_its_note_before_the_detach_returns() {
    let b = boot();
    let (nine, mut lend) = vm_console(&b);
    let one = Console::start(b.steward);
    call(b.steward, b.control, Message::Attach(Attach { note: "" }), &[one.handle]).unwrap();
    vm_write(&b, &nine, &mut lend, b"prompt> ");
    one.wait_for(b"prompt> ");
    // The client is slow, not gone: the chunk in flight moves 40 ms after the detach is sent.
    one.stall(b.steward, true);
    vm_write(&b, &nine, &mut lend, b"busy");
    std::thread::sleep(Duration::from_millis(50));
    let (steward, wake) = (b.steward, one.wake);
    let stalled = one.pipe.stalled.clone();
    let moves = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(40));
        stalled.store(false, Ordering::SeqCst);
        fake()
            .as_process(steward, || Endpoint::from_handle(wake).send(&[1, 0, 0, 0], &[], None, FOREVER))
            .expect("the console takes the wake");
    });
    let note = "[taken over]\r\n";
    call(b.steward, b.control, Message::Detach(Detach { note }), &[]).unwrap();
    moves.join().unwrap();
    assert_eq!(one.pipe.out.lock().unwrap().as_slice(), b"prompt> busy[taken over]\r\n");
    fake().as_process(b.vm, || drop((nine, lend)));
}

/// A channel's input ends (`ssh alice@box < file`, no pty): the VM reads the last line and then
/// the end of the file, and its output still reaches the channel.
#[test]
fn a_channel_s_input_end_reaches_the_vm_as_the_end_of_the_file() {
    let b = boot();
    let (nine, mut lend) = vm_console(&b);
    let one = Console::start(b.steward);
    call(b.steward, b.control, Message::Attach(Attach { note: "" }), &[one.handle]).unwrap();
    vm_write(&b, &nine, &mut lend, b"prompt> ");
    one.wait_for(b"prompt> ");
    one.end_input(b.steward, b"1 + 1\n");
    let mut out = [0u8; 16];
    let n = fake().as_process(b.vm, || nine.read(&mut lend, 0, 0, &mut out)).unwrap();
    assert_eq!(&out[..n], b"1 + 1\n");
    let n = fake().as_process(b.vm, || nine.read(&mut lend, 0, 0, &mut out)).unwrap();
    assert_eq!(n, 0, "the end of the file");
    vm_write(&b, &nine, &mut lend, b"2\n");
    one.wait_for(b"2\n");
    fake().as_process(b.vm, || drop((nine, lend)));
}

/// A `consol` call by the VM, as process `vm`, on its console `cons`: `size`, or `resize`, which
/// waits in the relay.
fn vm_consol(vm: usize, cons: Handle, message: consol::Message) -> (u16, u16) {
    fake().as_process(vm, || {
        let mut lend = Lend::new(1).unwrap();
        let size = redoubt_client::typed::call::<consol::Protocol, _>(
            &Endpoint::from_handle(cons),
            &mut lend,
            &message,
            &[],
            |r, _| match r {
                consol::Reply::Size(r) => Some((r.cols, r.rows)),
                consol::Reply::Resize(r) => Some((r.cols, r.rows)),
                consol::Reply::Ended(_) => None,
            },
        );
        drop(lend);
        size.expect("the relay answers").expect("a size")
    })
}

/// The VM's `resize`, waiting in a thread of its own.
fn vm_resize(b: &Box_) -> std::thread::JoinHandle<(u16, u16)> {
    let (vm, cons) = (b.vm, b.cons);
    std::thread::spawn(move || vm_consol(vm, cons, consol::Message::Resize(consol::Resize {})))
}

/// The VM's `size` is the attached channel's, `resize` waits in the relay for its terminal to
/// change, and an attach is a change: a VM waiting is told the new channel's size, so it redraws.
#[test]
fn the_vm_s_size_is_the_channel_s_and_an_attach_redraws() {
    let b = boot();
    let size = || vm_consol(b.vm, b.cons, consol::Message::Size(consol::Size {}));
    assert_eq!(size(), (80, 24), "none attached");
    let waiting = vm_resize(&b);
    let one = Console::start(b.steward);
    one.resize(b.steward, (100, 40));
    call(b.steward, b.control, Message::Attach(Attach { note: "" }), &[one.handle]).unwrap();
    assert_eq!(waiting.join().unwrap(), (100, 40), "the attach's first size is a change");
    assert_eq!(size(), (100, 40));
    let waiting = vm_resize(&b);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!waiting.is_finished(), "no change yet");
    one.resize(b.steward, (132, 43));
    assert_eq!(waiting.join().unwrap(), (132, 43));
}
