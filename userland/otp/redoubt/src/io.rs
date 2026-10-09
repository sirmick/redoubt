//! The VM's I/O on the client library's hub (docs/userland/beamlet.md, "Asynchronous underneath,
//! synchronous on top"; docs/userland/native.md, "Many requests at once").
//!
//! - **One hub per VM**, owned by the platform and so run by whichever scheduler holds it: a request is
//!   submitted inline, waiting at most the hub's `SUBMIT_TIMEOUT_US` for its server to take it, and its
//!   buffer is the hub's until its completion.
//! - **One waiter thread per connection**, started the first time the connection is used: it sits in the
//!   connection's completion call and hands what it brings to the VM's own endpoint, [`Io::wait`]'s, with one
//!   wake-up `send` a batch. The connections are the namespace's, at most one per binding.
//! - **Nothing waits but the VM's idle**: [`Io::wait`] is a `receive` on that endpoint with the VM's next
//!   deadline, and every wake-up goes to the hub, whose completions the platform takes in turn.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_client::Error;
use redoubt_client::aio::{COMPLETION_PAGES, Conn, Done, Hub, RETRY_US};
use redoubt_client::file::Connection;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Delivery, Event};

/// The most waiter threads the VM starts: one per connection of a session's namespace (bootfsd,
/// the home volume, a labelled volume, `ipd`, the console, the system volume); a bind is one of
/// these connections again. Each is a thread of the process, under the kernel's 255 a process. A
/// connection whose server has ended gives its place up ([`Io::connect`]).
pub const MAX_WAITERS: usize = 6;

/// The hub, its connections, and the endpoint its waiters wake the VM on.
pub struct Io {
    hub: Hub,
    wake: Endpoint,
    /// Each hub connection, by the namespace connection it is a session on, or why its waiter could
    /// not start, which stands: its session is not opened again; and its waiter's badge.
    conns: Vec<(Connection, Result<Conn, Error>, u64)>,
    /// Requests handed to the hub so far.
    requests: u64,
    /// Wake-ups that are not the hub's (the system calls' threads'), for the platform.
    others: VecDeque<Delivery>,
}

impl Io {
    pub fn new() -> Result<Io, Error> {
        Ok(Io {
            hub: Hub::new(),
            wake: Endpoint::create()?,
            conns: Vec::new(),
            requests: 0,
            others: VecDeque::new(),
        })
    }

    /// `conn`'s hub connection: the first time, a multiplexed session on it and its waiter. A server
    /// that serves no multiplexed session refuses the first completion call, and that is the error;
    /// a waiter that cannot start is one too, for good. At [`MAX_WAITERS`], a connection whose
    /// server has ended and whose waiter has returned gives its place up first: a session's `piped`
    /// is a new server, on a new connection, at every pipeline (docs/servers/piped.md).
    pub fn connect(&mut self, conn: &Connection) -> Result<Conn, Error> {
        if let Some((_, c, _)) = self.conns.iter().find(|(known, _, _)| known.same(conn)) {
            return *c;
        }
        if self.conns.len() == MAX_WAITERS {
            // A waiter's last wake-up may be waiting still: taken, its connection is over.
            self.take_waiting();
            let hub = &mut self.hub;
            self.conns.retain(|(_, c, _)| !c.is_ok_and(|c| hub.release(c)));
        }
        if self.conns.len() == MAX_WAITERS {
            return Err(Error::Sys(redoubt_rt::abi::Error::TooManyThreads));
        }
        // Badges from 1 to MAX_WAITERS, one no other waiter of the VM's holds: the waiter's
        // wake-ups are told apart by them, none is 0, and all are below the system calls' threads'.
        let free = (1..=MAX_WAITERS as u64).find(|b| !self.conns.iter().any(|(_, _, held)| *held == *b));
        let badge = free.and_then(NonZeroU64::new).ok_or(Error::Unexpected)?;
        let c = self.hub.connect(Endpoint::from_handle(conn.endpoint().handle()))?;
        let started = self.hub.spawn_waiter(c, &self.wake, badge).map(|()| c);
        self.conns.push((conn.clone(), started, badge.get()));
        started
    }

    /// The hub, for one request, which is counted.
    pub fn request(&mut self) -> &mut Hub {
        self.requests += 1;
        &mut self.hub
    }

    /// Requests handed to the hub so far.
    pub fn requests(&self) -> u64 { self.requests }

    /// The next completion, if any.
    pub fn completed(&mut self) -> Option<Done> { self.hub.completed() }

    /// Hands every wake-up already waiting to the hub, without waiting.
    pub fn take_waiting(&mut self) {
        while let Ok(event) = self.wake.receive(0, COMPLETION_PAGES) {
            self.take(event);
        }
    }

    /// Waits at most `timeout` µs for a wake-up, then hands it and every other waiting to the hub.
    /// While a request waits in the hub's queue for a busy server, the wait is at most the hub's
    /// `RETRY_US`, so the queue is tried again.
    pub fn wait(&mut self, timeout: u64) {
        self.hub.poll();
        let timeout = if self.hub.queued() > 0 { timeout.min(RETRY_US) } else { timeout };
        if let Ok(event) = self.wake.receive(timeout, COMPLETION_PAGES) {
            self.take(event);
            self.take_waiting();
        }
    }

    /// The VM's own endpoint, where every thread of the platform wakes it.
    pub fn wake(&self) -> &Endpoint { &self.wake }

    /// The next wake-up that was not the hub's, if any.
    pub fn other(&mut self) -> Option<Delivery> { self.others.pop_front() }

    /// How many waiter threads have started.
    pub fn waiters(&self) -> usize { self.hub.waiters() }

    fn take(&mut self, event: Event) {
        let Event::Send(delivery) = event else { return };
        // A waiter's wake-up is the hub's; any other is the platform's to take or close.
        if let Some(other) = self.hub.deliver(delivery) {
            self.others.push_back(other);
        }
    }
}
