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

use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_client::Error;
use redoubt_client::aio::{COMPLETION_PAGES, Conn, Done, Hub, RETRY_US};
use redoubt_client::file::Connection;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;

/// The most waiter threads the VM starts: one per namespace binding a session is given (bootfsd,
/// the home volume, a labelled volume, `ipd`, the console, the system volume), with room for a
/// few binds. Each is a thread of the process, under the kernel's 255 a process.
pub const MAX_WAITERS: usize = 8;

/// The hub, its connections, and the endpoint its waiters wake the VM on.
pub struct Io {
    hub: Hub,
    wake: Endpoint,
    /// Each hub connection, by the namespace connection it is a session on.
    conns: Vec<(Connection, Conn)>,
}

impl Io {
    pub fn new() -> Result<Io, Error> {
        Ok(Io { hub: Hub::new(), wake: Endpoint::create()?, conns: Vec::new() })
    }

    /// `conn`'s hub connection: the first time, a multiplexed session on it and its waiter. A server
    /// that serves no multiplexed session refuses the first completion call, and that is the error.
    pub fn connect(&mut self, conn: &Connection) -> Result<Conn, Error> {
        if let Some((_, c)) = self.conns.iter().find(|(known, _)| known.same(conn)) {
            return Ok(*c);
        }
        if self.conns.len() == MAX_WAITERS {
            return Err(Error::Sys(redoubt_rt::abi::Error::TooManyThreads));
        }
        let c = self.hub.connect(Endpoint::from_handle(conn.endpoint().handle()))?;
        // Badges from 1: the waiter's wake-ups are told apart by them, and none is 0.
        let badge = NonZeroU64::new(self.conns.len() as u64 + 1).ok_or(Error::Unexpected)?;
        self.hub.spawn_waiter(c, &self.wake, badge)?;
        self.conns.push((conn.clone(), c));
        Ok(c)
    }

    pub fn hub(&mut self) -> &mut Hub { &mut self.hub }

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

    /// How many waiter threads have started.
    pub fn waiters(&self) -> usize { self.hub.waiters() }

    fn take(&mut self, event: Event) {
        let Event::Send(delivery) = event else { return };
        // Nothing but a waiter sends here, and a wake-up carries no handle; anything else is closed.
        if let Some(other) = self.hub.deliver(delivery) {
            for handle in other.handles.as_slice().iter().flatten() {
                let _ = redoubt_rt::handle::close(*handle);
            }
        }
    }
}
