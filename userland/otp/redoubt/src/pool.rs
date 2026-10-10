//! The VM's typed calls (docs/userland/beamlet.md, "Natives"): a small pool of threads that make the
//! calls no hub carries, so no Erlang process, and no scheduler, waits for a server.
//!
//! - **At most [`CALL_THREADS`] calls are out at once**, one per thread, each started the first time it is
//!   needed; up to [`MAX_QUEUED`] more wait here in order, and past that a call is refused `busy`. A thread
//!   lives as long as the VM: its stack is spent once ([`redoubt_rt::thread::spawn`]).
//! - **A call is the caller's handles and pages, by number**: the thread is of the same process, so it uses
//!   the VM's own handles, which the VM holds until the call's end, and the call's lend goes to it and back
//!   as a transfer, as a hub waiter's buffer does.
//! - **The words go through a slot of atomics** the two share ([`Slot`]): the VM fills it and sends the
//!   thread one word; the thread makes the call, fills it with the outcome, and wakes the VM on its own
//!   endpoint through a badge minted for that thread ([`BADGE`] + its index).
//! - **Every call is bounded by its timeout**, at most the natives' 5 s: a server that never answers holds
//!   one thread that long, and no more.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use beamlet_vm::platform::{Message, Object, Refused};
use redoubt_rt::abi::{FOREVER, Handle, MAX_LEND_PAGES, MAX_MSG_HANDLES};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Delivery, Event};

use crate::system::{Cap, HAND_US, Life, kernel, worker};

/// The most threads making typed calls at once: a stated constant beside the hub's `MAX_WAITERS`.
pub const CALL_THREADS: usize = 2;

/// The most calls waiting for a thread; past it a call is refused `busy`.
pub const MAX_QUEUED: usize = 64;

/// The badge of call thread `i`'s wake-ups is `BADGE + i`: above every hub waiter's.
pub const BADGE: u64 = 0x100;

/// A call thread's stack, in pages.
const STACK_PAGES: usize = 4;

/// The word that starts a call, the one that says it has ended, and the first thread's first word.
const GO: u64 = 0x60;
const DONE: u64 = 0xd0e;
const HELLO: u64 = 0x4e110;

/// How long the platform waits for the first thread's first word at its start (µs).
const HELLO_US: u64 = 1_000_000;

/// What the VM and one call thread share, in 32-bit cells (the narrower width has no 64-bit
/// atomics): the call's target, timeout, words and handles going in; its status, the reply's words
/// and the handles it brought coming out.
struct Slot([AtomicU32; CELLS]);

const CELLS: usize = 36;
const TARGET: usize = 0;
const TIMEOUT: usize = 1;
const WORDS: usize = 3;
const HANDLES: usize = 11;
const COUNT: usize = 15;
const STATUS: usize = 16;
const REPLY: usize = 17;
const GOT: usize = 25;
const GOT_COUNT: usize = 29;

impl Slot {
    fn new() -> Slot { Slot(core::array::from_fn(|_| AtomicU32::new(0))) }

    fn get(&self, i: usize) -> u32 { self.0[i].load(Ordering::SeqCst) }

    fn set(&self, i: usize, v: u32) { self.0[i].store(v, Ordering::SeqCst) }

    fn get64(&self, i: usize) -> u64 { u64::from(self.get(i)) | u64::from(self.get(i + 1)) << 32 }

    fn set64(&self, i: usize, v: u64) {
        self.set(i, v as u32);
        self.set(i + 1, (v >> 32) as u32);
    }
}

/// A call waiting for a thread: who asked, the handles it names (checked by the platform), its
/// words and lend, its timeout, and the resources that keep its handles open until it ends.
pub(crate) struct Call {
    pub(crate) asker: u64,
    pub(crate) id: u64,
    pub(crate) to: Handle,
    pub(crate) carried: Vec<Handle>,
    pub(crate) words: [u64; 4],
    pub(crate) buffer: Option<Vec<u8>>,
    pub(crate) timeout_us: u64,
    pub(crate) held: Vec<Object>,
    /// What the handles its reply brings die with: its target's (`crate::system::Cap`).
    pub(crate) returns: Option<Arc<Life>>,
}

/// One call thread: its slot, where the VM sends it, and the call it is making.
struct Thread {
    slot: Arc<Slot>,
    go: Endpoint,
    out: Option<(u64, u64, Vec<Object>, bool, Option<Arc<Life>>)>,
    /// It did not take a call in time: it has ended, and takes no more.
    gone: bool,
}

/// The pool: its threads, the calls waiting, and the calls ended, for their askers.
#[derive(Default)]
pub(crate) struct Pool {
    threads: Vec<Thread>,
    queue: VecDeque<Call>,
    ended: VecDeque<(u64, u64, Result<Message, Refused>)>,
}

impl Pool {
    /// The pool, its first thread started, and this process's label set, read off that thread's
    /// first wake-up: the kernel stamps every message with its sender's labels, so this is the
    /// kernel's word, not a launcher's (docs/userland/beamlet.md, "Natives": `labels/0`). A send
    /// waits for its receiver, so it takes a second thread; the first call thread is it.
    pub(crate) fn start(wake: &Endpoint) -> Result<(Pool, Vec<u64>), Refused> {
        let thread = spawn(wake, 0, true)?;
        let labels = match wake.receive(HELLO_US, 0) {
            Ok(Event::Send(d)) if d.caller.badge == BADGE && d.words[0] == HELLO => {
                d.caller.labels.as_slice().to_vec()
            }
            _ => return Err(Refused("protocol")),
        };
        Ok((Pool { threads: alloc::vec![thread], ..Pool::default() }, labels))
    }

    /// Makes `call`, now or when a thread is free.
    pub(crate) fn call(&mut self, wake: &Endpoint, call: Call) -> Result<(), Refused> {
        if self.queue.len() >= MAX_QUEUED {
            return Err(Refused("busy"));
        }
        self.queue.push_back(call);
        self.hand_out(wake);
        Ok(())
    }

    /// Whether a call is out or waiting.
    pub(crate) fn busy(&self) -> bool {
        !self.queue.is_empty() || self.threads.iter().any(|t| t.out.is_some())
    }

    /// The next call ended: its asker, its id and how it went.
    pub(crate) fn ended(&mut self) -> Option<(u64, u64, Result<Message, Refused>)> { self.ended.pop_front() }

    /// Takes a call thread's wake-up; anything else is handed back.
    pub(crate) fn deliver(&mut self, wake: &Endpoint, delivery: Delivery) -> Option<Delivery> {
        let i = delivery.caller.badge.wrapping_sub(BADGE) as usize;
        if delivery.words[0] != DONE || i >= self.threads.len() {
            return Some(delivery);
        }
        let thread = &mut self.threads[i];
        let Some((asker, id, held, lent, returns)) = thread.out.take() else { return Some(delivery) };
        let slot = &thread.slot;
        let status = slot.get(STATUS);
        let result = if status != 0 {
            Err(Refused(abi_name(status)))
        } else {
            let words = core::array::from_fn(|w| slot.get64(REPLY + 2 * w));
            let got = (slot.get(GOT_COUNT) as usize).min(MAX_MSG_HANDLES);
            let handles = (0..got)
                .filter_map(|h| Handle::new(slot.get(GOT + h)))
                .map(|h| Cap::returned(h, returns.clone()))
                .collect();
            // A lent call's reply is the bytes its word 1 says, at the front of the lend; an error
            // reply has none.
            let buffer = lent.then(|| {
                let bytes = delivery.transfer.as_deref().unwrap_or(&[]);
                let n = if words[0] == 0 { (words[1] as usize).min(bytes.len()) } else { 0 };
                bytes[..n].to_vec()
            });
            Ok(Message { words, buffer, handles })
        };
        // The handles the call carried may close now.
        drop(held);
        self.ended.push_back((asker, id, result));
        self.hand_out(wake);
        None
    }

    /// Hands waiting calls to free threads, starting one if none is free and fewer than
    /// [`CALL_THREADS`] run. A call that cannot be sent ends at once with why.
    fn hand_out(&mut self, wake: &Endpoint) {
        while let Some(call) = self.queue.pop_front() {
            let free = match self.threads.iter().position(|t| t.out.is_none() && !t.gone) {
                Some(i) => i,
                None if self.threads.iter().filter(|t| !t.gone).count() < CALL_THREADS => {
                    match spawn(wake, self.threads.len(), false) {
                        Ok(thread) => {
                            self.threads.push(thread);
                            self.threads.len() - 1
                        }
                        Err(e) => {
                            self.ended.push_back((call.asker, call.id, Err(e)));
                            continue;
                        }
                    }
                }
                None => {
                    self.queue.push_front(call);
                    return;
                }
            };
            if let Err(e) = self.send(free, call) {
                let (asker, id) = e.0;
                self.ended.push_back((asker, id, Err(e.1)));
            }
        }
    }

    /// Puts `call` out on thread `i`.
    fn send(&mut self, i: usize, call: Call) -> Result<(), ((u64, u64), Refused)> {
        let who = (call.asker, call.id);
        let thread = &mut self.threads[i];
        let slot = &thread.slot;
        slot.set(TARGET, call.to.index());
        slot.set64(TIMEOUT, call.timeout_us);
        for (w, word) in call.words.iter().enumerate() {
            slot.set64(WORDS + 2 * w, *word);
        }
        let carried = &call.carried[..call.carried.len().min(MAX_MSG_HANDLES)];
        for (h, handle) in carried.iter().enumerate() {
            slot.set(HANDLES + h, handle.index());
        }
        slot.set(COUNT, carried.len() as u32);
        let lend = match &call.buffer {
            None => None,
            Some(bytes) => {
                let mut pages = Buffer::new(MAX_LEND_PAGES).map_err(|e| (who, Refused(kernel(e))))?;
                pages.get_mut(..bytes.len()).ok_or((who, Refused("too_large")))?.copy_from_slice(bytes);
                Some(pages)
            }
        };
        let lent = lend.is_some();
        if thread.go.send(&[GO, 0, 0, 0], &[], lend, HAND_US).is_err() {
            thread.gone = true;
            return Err((who, Refused("protocol")));
        }
        thread.out = Some((call.asker, call.id, call.held, lent, call.returns));
        Ok(())
    }
}

/// The kernel's error by its number, as the slot carries it.
fn abi_name(code: u32) -> &'static str {
    redoubt_rt::abi::Error::ALL.iter().find(|e| **e as u32 == code).map_or("protocol", |e| kernel(*e))
}

/// Starts call thread `i`; with `hello`, it first says so on the VM's endpoint.
fn spawn(wake: &Endpoint, i: usize, hello: bool) -> Result<Thread, Refused> {
    let slot = Arc::new(Slot::new());
    let shared = Arc::clone(&slot);
    let go = worker(wake, BADGE + i as u64, STACK_PAGES, move |receive, done| {
        serve(receive, done, shared, hello)
    })?;
    Ok(Thread { slot, go, out: None, gone: false })
}

/// A call thread: takes each call the VM sends, makes it, and says how it went.
fn serve(receive: Endpoint, done: Endpoint, slot: Arc<Slot>, hello: bool) {
    if hello && done.send(&[HELLO, 0, 0, 0], &[], None, HELLO_US).is_err() {
        return;
    }
    loop {
        let lend = match receive.receive(FOREVER, MAX_LEND_PAGES) {
            Ok(Event::Send(delivery)) if delivery.words[0] == GO => delivery.transfer,
            Ok(_) => continue,
            Err(_) => return,
        };
        let to = Handle::new(slot.get(TARGET));
        let words = core::array::from_fn(|w| slot.get64(WORDS + 2 * w));
        let count = (slot.get(COUNT) as usize).min(MAX_MSG_HANDLES);
        let handles: Vec<Handle> = (0..count).filter_map(|h| Handle::new(slot.get(HANDLES + h))).collect();
        let back = match to {
            None => {
                slot.set(STATUS, redoubt_rt::abi::Error::BadHandle as u32);
                lend
            }
            Some(to) => {
                let mut outcome = Endpoint::from_handle(to).call(&words, &handles, lend, slot.get64(TIMEOUT));
                match (&outcome.status, outcome.reply.take()) {
                    (Ok(()), Some(reply)) => {
                        slot.set(STATUS, 0);
                        for (w, word) in reply.words.iter().enumerate() {
                            slot.set64(REPLY + 2 * w, *word);
                        }
                        let got = reply.handles.as_slice();
                        for (h, handle) in got.iter().enumerate() {
                            slot.set(GOT + h, handle.map_or(0, |h| h.index()));
                        }
                        slot.set(GOT_COUNT, got.len() as u32);
                    }
                    (Err(e), _) => slot.set(STATUS, *e as u32),
                    (Ok(()), None) => slot.set(STATUS, redoubt_rt::abi::Error::InvalidArgument as u32),
                }
                outcome.buffer.take()
            }
        };
        if done.send(&[DONE, 0, 0, 0], &[], back, FOREVER).is_err() {
            return;
        }
    }
}
