//! `consrelay`, the program: four threads over one [`Relay`] (servers/consrelay.md).
//!
//! - **The serving thread** owns the [`Relay`] and the 9P skeleton. It serves the VM's `/dev/cons`, parking a
//!   read with no input and a write with no room as `consoled` and `sshd` do, the steward's `attach` and
//!   `detach` on the control badge, and the two helper threads' calls. It makes no call that waits on
//!   anything but its own endpoint, so neither the VM nor the steward ever waits on a channel's console
//!   through it.
//! - **The writer thread** opens each channel's console as it is attached, then asks for output and writes it
//!   there, for as long as the channel is the context's. A slow channel holds this thread, and through the
//!   bound the VM's writes, never the serving thread.
//! - **The reader thread** reads what is typed on the channel the writer opened and hands it over.
//! - **The sizer thread** asks that channel's console its size (`consol`), then keeps a `resize` waiting
//!   there and hands over each change, so the VM's own `size` and `resize` are answered here, from what the
//!   serving thread last heard.
//!
//! The helper threads call the serving thread's endpoint through badges of their own ([`READER`],
//! [`WRITER`], [`SIZER`]), naming in each call the channel's generation, so a thread still on a channel the
//! steward has let go is told so ([`GONE`]) and leaves it. A channel's console handle is the serving
//! thread's; it is closed once no thread is on it ([`Relay::closable`]). The threads and their stacks are
//! made once, at the start.
//!
//! **The start.** The relay makes its endpoint in its own budget, the context's, mints the control badge and
//! the VM's console on it, and sends both to the steward on the hello badge its startup block holds
//! ([`HELLO`]). Its labels, which the VM's file carries, are those the kernel stamps on its own threads'
//! first calls: the relay runs in the context's budget, as the VM does.

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use alloc::string::String;
use core::num::NonZeroU64;

use redoubt_client::typed;
use redoubt_consrelay::{File, LIMITS, NOTE_CAP, Relay, Take, Thread, qid};
use redoubt_rt::abi::{Error, FOREVER, Handle, Handles, Labels, MAX_LEND_PAGES};
use redoubt_rt::client::{Connection as Nine, Lend};
use redoubt_rt::handle::{Endpoint, close};
use redoubt_rt::ipc::{Caller, Delivery, Event, Request, Words};
use redoubt_rt::server::minted::Minter;
use redoubt_rt::server::ninep::{Around, NineError, NineServer, WORDS_9P, mode, refuse, refuse_malformed};
use redoubt_rt::server::parked::{NotParked, Parked};
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::server::{close_delivery, consol as consol_server};
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::proto::{consol, consrelay};

redoubt_rt::entry!(serve);

/// The name of the hello badge in the startup block: the steward's, for the relay's one message.
pub const HELLO: &str = "hello";

/// The startup block named no hello badge.
pub const NO_HELLO: u32 = 2;
/// The relay's endpoint, its badges, its buffers or its threads could not be made.
pub const NO_START: u32 = 3;
/// The helper threads did not call within [`START_US`].
pub const NO_THREADS: u32 = 4;
/// The steward did not take the hello within [`START_US`].
pub const NOT_TAKEN: u32 = 5;

/// The steward's control badge.
pub const CONTROL: u64 = 1;
/// The reader thread's badge.
pub const READER: u64 = 2;
/// The writer thread's badge.
pub const WRITER: u64 = 3;
/// The sizer thread's badge.
pub const SIZER: u64 = 5;
/// The badge the relay mints the VM's console as: its own, account 0.
const MAIN: u64 = 4;

/// A helper's call: word 0 is the operation, word 1 the channel's generation.
/// `NEXT`: the next channel, newer than the last; the reply's words 1 and 2 are its generation
/// and handle.
pub const NEXT: u64 = 1;
/// `OPENED`: the writer opened the channel (word 2: 1) or could not (0).
pub const OPENED: u64 = 2;
/// `OUTPUT`: output to write, into the lend; the reply's word 1 is how many bytes.
pub const OUTPUT: u64 = 3;
/// `INPUT`: word 2 bytes typed, in the lend; the reply's word 1 is how many were taken.
pub const INPUT: u64 = 4;
/// `BROKEN`: a call on the channel failed.
pub const BROKEN: u64 = 5;
/// `SIZE`: the channel's console is word 2 columns by word 3 rows.
pub const SIZE: u64 = 6;
/// `ENDED`: the channel's input ended (a read found the end of the file).
pub const ENDED: u64 = 7;
/// Word 0 of a reply: the channel is let go.
pub const GONE: u64 = 1;

/// How long the start waits for its threads' first calls, and for the steward to take the
/// hello (µs).
pub const START_US: u64 = 2_000_000;
/// A helper thread's stack, in pages.
const STACK_PAGES: usize = 4;
/// The bytes a helper moves at once: well inside one page's lend with a 9P header.
const CHUNK: usize = 2048;
/// The longest a `detach` waits for its note (µs): a writer in the middle of a chunk finishes it
/// and writes the note well within this on a channel that reads; one still in its write past it
/// is on a channel that has stopped reading, and the call is answered without the note.
pub const NOTE_WAIT_US: u64 = 150_000;
/// The writer's fid for the console, and the reader's, walked from it.
const WRITE_FID: u32 = 1;
const READ_FID: u32 = 2;

fn badge(b: u64) -> Result<NonZeroU64, Error> { NonZeroU64::new(b).ok_or(Error::InvalidArgument) }

fn me() -> Caller { Caller { badge: MAIN, account: 0, labels: Labels::new() } }

/// Mints the VM's console on the relay's endpoint.
struct Own<'e>(&'e Endpoint);

impl Minter for Own<'_> {
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> { Ok(self.0.mint(badge, None)?.handle()) }

    fn random(&mut self) -> Result<u64, Error> { redoubt_rt::handle::random_u64() }
}

/// A helper's call on the serving thread: the reply's words, or `None` if the call failed,
/// which only the relay's end makes happen.
fn ask(relay: &Endpoint, lend: &mut Lend, words: &Words) -> Option<Words> {
    let outcome = lend.call(relay, words, &[], FOREVER);
    outcome.status.ok()?;
    outcome.reply.as_ref().map(|r| r.words)
}

/// The writer: opens each channel, then writes what the relay gives it until the channel is let
/// go or a write fails.
fn writer(relay: Endpoint) {
    let (Ok(mut lend), Ok(mut console)) = (Lend::new(1), Lend::new(1)) else { return };
    let mut chunk = [0u8; CHUNK];
    loop {
        let Some(next) = ask(&relay, &mut lend, &[NEXT, 0, 0, 0]) else { return };
        let (generation, handle) = (next[1], next[2] as u32);
        let Some(handle) = Handle::new(handle) else { continue };
        let nine = Nine::new(Endpoint::from_handle(handle));
        let opened = nine.version(&mut console).is_ok()
            && nine.attach(&mut console, WRITE_FID, "").is_ok()
            && nine.walk(&mut console, WRITE_FID, READ_FID, "").is_ok()
            && nine.open(&mut console, WRITE_FID, mode::OWRITE).is_ok()
            && nine.open(&mut console, READ_FID, mode::OREAD).is_ok();
        if ask(&relay, &mut lend, &[OPENED, generation, u64::from(opened), 0]).is_none() {
            return;
        }
        if !opened {
            continue;
        }
        loop {
            let Some(reply) = ask(&relay, &mut lend, &[OUTPUT, generation, 0, 0]) else { return };
            if reply[0] == GONE {
                break;
            }
            let n = (reply[1] as usize).min(CHUNK).min(lend.bytes().len());
            chunk[..n].copy_from_slice(&lend.bytes()[..n]);
            if !write_all(&nine, &mut console, &chunk[..n]) {
                if ask(&relay, &mut lend, &[BROKEN, generation, 0, 0]).is_none() {
                    return;
                }
                break;
            }
        }
    }
}

/// Writes all of `bytes` to the console, or says it could not.
fn write_all(nine: &Nine, lend: &mut Lend, bytes: &[u8]) -> bool {
    let mut done = 0;
    while done < bytes.len() {
        match nine.write(lend, WRITE_FID, 0, &bytes[done..]) {
            Ok(0) | Err(_) => return false,
            Ok(n) => done += n,
        }
    }
    true
}

/// The reader: reads what is typed on each channel the writer opened, and hands it over, until
/// the channel is let go, its input ends, which it says (`ENDED`), or a read fails (`BROKEN`).
fn reader(relay: Endpoint) {
    let (Ok(mut lend), Ok(mut console)) = (Lend::new(1), Lend::new(1)) else { return };
    let mut chunk = [0u8; CHUNK];
    loop {
        let Some(next) = ask(&relay, &mut lend, &[NEXT, 0, 0, 0]) else { return };
        let (generation, handle) = (next[1], next[2] as u32);
        let Some(handle) = Handle::new(handle) else { continue };
        let nine = Nine::new(Endpoint::from_handle(handle));
        'channel: loop {
            let n = match nine.read(&mut console, READ_FID, 0, &mut chunk) {
                Ok(n) if n > 0 => n,
                read => {
                    let op = if read.is_ok() { ENDED } else { BROKEN };
                    if ask(&relay, &mut lend, &[op, generation, 0, 0]).is_none() {
                        return;
                    }
                    break;
                }
            };
            let mut given = 0;
            while given < n {
                let Ok(pages) = lend.pages() else { return };
                let m = (n - given).min(pages.len());
                pages[..m].copy_from_slice(&chunk[given..given + m]);
                let Some(reply) = ask(&relay, &mut lend, &[INPUT, generation, m as u64, 0]) else { return };
                if reply[0] == GONE {
                    break 'channel;
                }
                given += (reply[1] as usize).min(m);
            }
        }
    }
}

/// The sizer: on each channel the writer opened, asks its console's size, then waits there for
/// each change (`consol`'s `resize`) and hands each answer over, until the channel is let go or its
/// console stops answering, as `sshd`'s does once the channel's session has ended.
fn sizer(relay: Endpoint) {
    let (Ok(mut lend), Ok(mut console)) = (Lend::new(1), Lend::new(1)) else { return };
    loop {
        let Some(next) = ask(&relay, &mut lend, &[NEXT, 0, 0, 0]) else { return };
        let (generation, handle) = (next[1], next[2] as u32);
        let Some(handle) = Handle::new(handle) else { continue };
        let channel = Endpoint::from_handle(handle);
        let mut message = consol::Message::Size(consol::Size {});
        loop {
            let size =
                typed::call::<consol::Protocol, _>(&channel, &mut console, &message, &[], |r, _| match r {
                    consol::Reply::Size(r) => Some((r.cols, r.rows)),
                    consol::Reply::Resize(r) => Some((r.cols, r.rows)),
                    consol::Reply::Ended(_) => None,
                });
            let Ok(Some((cols, rows))) = size else { break };
            let words = [SIZE, generation, u64::from(cols), u64::from(rows)];
            let Some(reply) = ask(&relay, &mut lend, &words) else { return };
            if reply[0] == GONE {
                break;
            }
            message = consol::Message::Resize(consol::Resize {});
        }
    }
}

/// The calls the serving thread holds beside the VM's parked ones: each helper's call that waits
/// for something to move, and the steward's `detach` while its note is written, until
/// `detach_due`.
#[derive(Default)]
struct Waits {
    next: [Option<Request>; 3],
    output: Option<Request>,
    input: Option<Request>,
    detach: Option<Request>,
    detach_due: u64,
}

/// What a parked VM call waits for: input or room, or a change of the console's size since the
/// count it saw.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Io,
    Resize(u64),
}

/// What the serving thread keeps beside the skeleton.
struct Serving {
    parked: Parked<Waiting>,
    waits: Waits,
}

fn reply(request: Request, words: Words) {
    let _ = finish(request, &Outcome { words, send: Handles::new(), close: Handles::new() });
}

/// The reply to a control call that succeeded.
fn done(request: Request, answer: consrelay::Reply) {
    let mut request = request;
    match answer.encode(request.lend()) {
        Ok(words) => reply(request, words),
        Err(_) => {
            let _ = refuse_malformed(request);
        }
    }
}

impl Serving {
    /// Answers the VM's `request`, or parks it if it must wait: a read or write the file cannot
    /// answer yet, or `consol`'s `resize`, which waits for the console's size to change.
    fn vm(&mut self, server: &mut NineServer<Relay>, request: Request, now: u64) {
        let size = server.fs.size();
        let held = server.serve_parking(request, |_, r| match consol_server::asks(&r.words) {
            true => consol_server::serve(size, r),
            false => refuse_malformed(r).map(|()| None),
        });
        let Ok(Some(request)) = held else { return };
        let waiting = match consol_server::asks(&request.words) {
            true => Waiting::Resize(server.fs.resized()),
            false => Waiting::Io,
        };
        let charge = server.charge_of(&request.caller);
        if let Err(NotParked(request)) =
            self.parked.park(server.admission_mut(), request, charge, waiting, now)
        {
            let _ = match waiting {
                Waiting::Io => refuse(request, NineError::TOO_MANY),
                Waiting::Resize(_) => refuse_malformed(request),
            };
        }
    }

    /// The steward's `attach` and `detach`. Anything else on the control badge is malformed.
    fn control(&mut self, server: &mut NineServer<Relay>, mut request: Request, now: u64) {
        let (words, count) = (request.words, request.handles.as_slice().len());
        let console = request.handles.as_slice().first().copied().flatten();
        let mut note = String::new();
        let message = match consrelay::Message::decode(&words, request.lend(), count) {
            Ok(consrelay::Message::Attach(a)) if note.try_reserve(a.note.len().min(NOTE_CAP)).is_ok() => {
                note.push_str(a.note);
                Some(true)
            }
            Ok(consrelay::Message::Detach(d)) if note.try_reserve(d.note.len().min(NOTE_CAP)).is_ok() => {
                note.push_str(d.note);
                Some(false)
            }
            _ => None,
        };
        // A detach still waiting on its note is answered: the channel is let go either way.
        if let Some(held) = self.waits.detach.take() {
            done(held, consrelay::Reply::Detach(consrelay::DetachReply {}));
        }
        match (message, console) {
            (Some(true), Some(console)) => {
                server.fs.attach(console.index(), &note);
                done(request, consrelay::Reply::Attach(consrelay::AttachReply {}));
            }
            // The call waits for the note at most `NOTE_WAIT_US`: past that the writer is on a
            // channel that has stopped reading, and writes the note, if ever, after the call is
            // answered.
            (Some(false), _) => {
                if server.fs.detach(&note) {
                    self.waits.detach = Some(request);
                    self.waits.detach_due = now.saturating_add(NOTE_WAIT_US);
                } else {
                    done(request, consrelay::Reply::Detach(consrelay::DetachReply {}));
                }
            }
            _ => {
                let _ = refuse_malformed(request);
            }
        }
    }

    /// A helper's call: answered at once, or held until something moves.
    fn helper(&mut self, server: &mut NineServer<Relay>, thread: Thread, request: Request) {
        let (op, generation) = (request.words[0], request.words[1]);
        match (op, thread) {
            (NEXT, _) => self.waits.next[thread as usize] = Some(request),
            (OPENED, Thread::Writer) => {
                server.fs.opened(generation, request.words[2] != 0);
                reply(request, [0; 4]);
            }
            (OUTPUT, Thread::Writer) => self.waits.output = Some(request),
            (INPUT, Thread::Reader) => self.waits.input = Some(request),
            (BROKEN, _) => {
                server.fs.broken(generation);
                reply(request, [0; 4]);
            }
            (ENDED, Thread::Reader) => {
                server.fs.input_ended(generation);
                reply(request, [0; 4]);
            }
            (SIZE, Thread::Sizer) => {
                let size = (request.words[2] as u16, request.words[3] as u16);
                let current = server.fs.sized(generation, size);
                reply(request, [if current { 0 } else { GONE }, 0, 0, 0]);
            }
            _ => {
                let _ = refuse_malformed(request);
            }
        }
    }

    /// Answers every held call that can be answered now.
    fn settle(&mut self, server: &mut NineServer<Relay>, now: u64) {
        for thread in [Thread::Writer, Thread::Reader, Thread::Sizer] {
            let slot = &mut self.waits.next[thread as usize];
            if slot.is_some() {
                if let Some(channel) = server.fs.next(thread) {
                    if let Some(request) = slot.take() {
                        reply(request, [0, channel.generation, u64::from(channel.handle), 0]);
                    }
                }
            }
        }
        if let Some(mut request) = self.waits.output.take() {
            let generation = request.words[1];
            // No more than the writer copies out of its lend: the rest would be lost.
            let lend = request.lend();
            let room = lend.len().min(CHUNK);
            match server.fs.take_output(generation, &mut lend[..room]) {
                Take::Bytes(n) => reply(request, [0, n as u64, 0, 0]),
                Take::Gone => reply(request, [GONE, 0, 0, 0]),
                Take::Wait => self.waits.output = Some(request),
            }
        }
        if let Some(mut request) = self.waits.input.take() {
            let (generation, n) = (request.words[1], request.words[2] as usize);
            let lend = request.lend();
            let bytes = &lend[..n.min(lend.len())];
            match server.fs.give_input(generation, bytes) {
                redoubt_consrelay::Give::Taken(0) => self.waits.input = Some(request),
                redoubt_consrelay::Give::Taken(k) => reply(request, [0, k as u64, 0, 0]),
                redoubt_consrelay::Give::Gone => reply(request, [GONE, 0, 0, 0]),
            }
        }
        if !server.fs.farewell_pending() || now >= self.waits.detach_due {
            if let Some(held) = self.waits.detach.take() {
                done(held, consrelay::Reply::Detach(consrelay::DetachReply {}));
            }
        }
        for handle in server.fs.closable() {
            if let Some(handle) = Handle::new(handle) {
                let _ = close(handle);
            }
        }
    }
}

impl Around<Relay> for Serving {
    fn call(&mut self, server: &mut NineServer<Relay>, request: Request, now: u64) {
        match request.caller.badge {
            CONTROL => self.control(server, request, now),
            READER => self.helper(server, Thread::Reader, request),
            WRITER => self.helper(server, Thread::Writer, request),
            SIZER => self.helper(server, Thread::Sizer, request),
            _ => self.vm(server, request, now),
        }
    }

    /// The helpers' calls first, then the VM's parked ones, which what the helpers moved may
    /// answer: each `resize` whose size has changed since it parked, with the size now, and every
    /// read and write again; then the helpers' again, which what the VM moved may answer.
    fn turn(&mut self, server: &mut NineServer<Relay>, now: u64) {
        self.settle(server, now);
        let (size, resized) = (server.fs.size(), server.fs.resized());
        let due = |w: &Waiting| matches!(w, Waiting::Resize(from) if *from != resized);
        while let Some(call) = self.parked.resume_first(server.admission_mut(), due) {
            if let Ok((request, _)) = call {
                let _ = consol_server::reply_resize(request, size);
            }
        }
        for _ in 0..self.parked.len() {
            let io = |w: &Waiting| *w == Waiting::Io;
            let Some(call) = self.parked.resume_first(server.admission_mut(), io) else { break };
            let Ok((request, _)) = call else { continue };
            self.vm(server, request, now);
        }
        self.settle(server, now);
        server.wake(now);
    }

    /// A caller gave up: a parked VM call, or the steward's `detach` past its bound.
    fn abandoned(&mut self, server: &mut NineServer<Relay>, id: NonZeroU64) {
        if self.waits.detach.as_ref().is_some_and(|r| r.id() == id) {
            if let Some(held) = self.waits.detach.take() {
                reply(held, [0; 4]);
            }
            return;
        }
        self.parked.abandoned(server.admission_mut(), id, &WORDS_9P);
    }

    fn send(&mut self, _: &mut NineServer<Relay>, delivery: Delivery, _: u64) { close_delivery(&delivery); }
}

/// The first helper call: its thread, its labels, and the call itself, held.
fn first_call(endpoint: &Endpoint, deadline: u64) -> Option<(Thread, Labels, Request)> {
    loop {
        let now = redoubt_rt::handle::time_now().ok()?;
        let wait = deadline.checked_sub(now).filter(|w| *w > 0)?;
        match endpoint.receive(wait, MAX_LEND_PAGES) {
            Ok(Event::Call(r)) if r.caller.badge == READER => {
                return Some((Thread::Reader, r.caller.labels, r));
            }
            Ok(Event::Call(r)) if r.caller.badge == WRITER => {
                return Some((Thread::Writer, r.caller.labels, r));
            }
            Ok(Event::Call(r)) if r.caller.badge == SIZER => {
                return Some((Thread::Sizer, r.caller.labels, r));
            }
            Ok(Event::Call(r)) => {
                let _ = refuse_malformed(r);
            }
            Ok(Event::Send(d)) => close_delivery(&d),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}

/// Starts the relay and serves until its endpoint is gone.
pub fn serve(startup: &Startup) -> u32 {
    let Some(hello) = startup.handle(HELLO).map(Endpoint::from_handle) else { return NO_HELLO };
    let made = Endpoint::create().and_then(|endpoint| {
        let control = endpoint.mint(badge(CONTROL)?, None)?;
        let reader_badge = endpoint.mint(badge(READER)?, None)?;
        let writer_badge = endpoint.mint(badge(WRITER)?, None)?;
        let sizer_badge = endpoint.mint(badge(SIZER)?, None)?;
        Ok((endpoint, control, reader_badge, writer_badge, sizer_badge))
    });
    let Ok((endpoint, control, reader_badge, writer_badge, sizer_badge)) = made else { return NO_START };
    let spawned =
        redoubt_rt::thread::spawn(alloc::boxed::Box::new(move || writer(writer_badge)), STACK_PAGES)
            .and_then(|_| {
                redoubt_rt::thread::spawn(alloc::boxed::Box::new(move || reader(reader_badge)), STACK_PAGES)
            })
            .and_then(|_| {
                redoubt_rt::thread::spawn(alloc::boxed::Box::new(move || sizer(sizer_badge)), STACK_PAGES)
            });
    if spawned.is_err() {
        return NO_START;
    }
    let deadline = redoubt_rt::handle::time_now().unwrap_or(0).saturating_add(START_US);
    let Some((thread, labels, first)) = first_call(&endpoint, deadline) else { return NO_THREADS };
    let Ok(relay) = Relay::new(labels.as_slice().to_vec()) else { return NO_START };
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_START };
    let Ok(mut server) = NineServer::new(relay, LIMITS, random) else { return NO_START };
    server.requests_wait(FOREVER);
    let Ok((vm, _, _)) = server.mint_rooted(&me(), (File, qid()), &mut Own(&endpoint)) else {
        return NO_START;
    };
    let mut words = [0; 4];
    let encoded = consrelay::Message::Hello(consrelay::Hello {}).encode(&mut []).map(|w| words = w);
    let sent = encoded.is_ok() && hello.send(&words, &[vm, control.handle()], None, START_US).is_ok();
    // The steward holds its copies now; the relay keeps neither.
    let _ = close(vm);
    let _ = close(control.handle());
    let _ = close(hello.handle());
    if !sent {
        return NOT_TAKEN;
    }
    // Every buffer is reserved by now. A session's relay has no console, so only a relay `init`
    // starts says so: `consrelay-footprint` waits for the line before the memory scan.
    redoubt_rt::start::say(startup, "consrelay: serving\n");
    let mut serving = Serving { parked: Parked::new(FOREVER), waits: Waits::default() };
    serving.waits.next[thread as usize] = Some(first);
    run(&mut server, &endpoint, serving)
}

/// The skeleton's loop (`NineServer::run_around`), which also wakes for a held `detach`'s bound.
fn run(server: &mut NineServer<Relay>, endpoint: &Endpoint, mut serving: Serving) -> u32 {
    loop {
        let now = redoubt_rt::handle::time_now().unwrap_or(0);
        server.expire(now);
        serving.turn(server, now);
        let held = serving.waits.detach.as_ref().map(|_| serving.waits.detach_due);
        let next = server.next_deadline().into_iter().chain(held).min();
        let timeout = next.map_or(FOREVER, |d| d.saturating_sub(now).max(1));
        let received = endpoint.receive(timeout, MAX_LEND_PAGES);
        let now = redoubt_rt::handle::time_now().unwrap_or(0);
        match received {
            Ok(Event::Call(request)) => serving.call(server, request, now),
            Ok(Event::Send(delivery)) => {
                if let Some(other) = server.deliver(delivery, now) {
                    serving.send(server, other, now);
                }
            }
            Ok(Event::Abandoned(id)) => {
                if !server.abandoned(id) {
                    serving.abandoned(server, id);
                }
            }
            Ok(Event::Interrupt | Event::Exit(_)) | Err(Error::Timeout) => {}
            Err(Error::Dead) => return redoubt_rt::exit::OK,
            Err(_) => return redoubt_rt::exit::RECEIVE_FAILED,
        }
    }
}
