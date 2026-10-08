//! Launching native programs (docs/userland/native.md, "Launching from a session"): the client
//! library's `Launch` from what the Erlang caller gave, and each job's one exit notice back to it as
//! an event.
//!
//! - **Everything the child gets is on the call**: the image the caller read, the budget it carved, its
//!   namespace entries and named handles, its arguments. The platform adds the loader stub, which it carries
//!   as `init` does, and the job's own exit endpoint, since `process_create` gives no PID to tell two
//!   children's notices apart.
//! - **A server is given its endpoint here.** With `serve`, the platform makes an endpoint, gives the child
//!   its receive right under that name and closes its own once the child has started, so the child alone
//!   serves it; the caller gets a send right ([`SERVED_BADGE`]), the one connection to it that nobody minted.
//! - **At most [`MAX_JOBS`] jobs run at once**, each watched by one of as many threads, started the first
//!   time one is needed and reused: a thread receives on its job's exit endpoint until the notice comes and
//!   wakes the VM with it through a badge minted for it ([`BADGE`] + its index). The VM then closes the job's
//!   process handle and exit endpoint; the budget is the caller's.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::num::NonZeroU64;
use core::sync::atomic::{AtomicU32, Ordering};

use beamlet_vm::platform::{Event, Launch, Object, Refused};
use redoubt_client::launch;
use redoubt_rt::abi::{Cause, FOREVER, Handle};
use redoubt_rt::handle::{Budget, Endpoint};
use redoubt_rt::ipc::{Delivery, Event as IpcEvent};

use crate::system::{HAND_US, kernel, name, worker};

/// The most jobs one VM runs at once: a pipeline's stages and the server of its pipes among them.
pub const MAX_JOBS: usize = 16;

/// The badge of the send right a launch with `serve` gives its caller: below the serving library's
/// first minted badge, so the server's own root connection.
pub const SERVED_BADGE: u64 = 1;

/// The badge of job thread `i`'s wake-ups is `BADGE + i`: above every serve thread's.
pub const BADGE: u64 = 0x300;

/// A job thread's stack, in pages.
const STACK_PAGES: usize = 4;

/// The words of a job thread's start, and of its notice.
const WATCH: u64 = 0x3a7c;
const ENDED: u64 = 0xe4d;

/// The loader stub every child is placed with (servers/init.md, "Launching through the loader
/// stub"), built for the target as `init` builds it; none on a host, whose fake kernel runs nothing.
#[cfg(target_os = "none")]
static STUB: &[u8] = include_bytes!(env!("STUB_BIN"));
#[cfg(not(target_os = "none"))]
static STUB: &[u8] = b"stub";

/// A running job: its asker, its reference, its exit endpoint and process, and what its launch was
/// given (closed or let go when it ends).
struct Job {
    asker: u64,
    job: u64,
    exit: Endpoint,
    process: Handle,
    held: Vec<Object>,
}

/// One job thread: the exit endpoint it is to watch, and the job it watches.
struct Thread {
    watch: Arc<AtomicU32>,
    go: Endpoint,
    job: Option<Job>,
    /// It did not take a job in time: it has ended, and takes no more.
    gone: bool,
}

/// A launch's handles, as the platform checked them: its budget, and each namespace entry's and
/// named handle's, in the launch's order.
pub(crate) struct Resolved {
    pub(crate) budget: Handle,
    pub(crate) namespace: Vec<Handle>,
    pub(crate) handles: Vec<Handle>,
}

#[derive(Default)]
pub(crate) struct Jobs {
    threads: Vec<Thread>,
}

impl Jobs {
    /// Whether a job runs.
    pub(crate) fn busy(&self) -> bool { self.threads.iter().any(|t| t.job.is_some()) }

    /// Starts `l` for `asker` as the job `job`, its budget, namespace entries and named handles
    /// already resolved by the platform ([`Resolved`]); with `l.serve`, the send right to the
    /// endpoint the child serves comes back.
    pub(crate) fn launch(
        &mut self,
        wake: &Endpoint,
        asker: u64,
        job: u64,
        l: Launch,
        resolved: Resolved,
    ) -> Result<Option<Endpoint>, Refused> {
        let i = match self.threads.iter().position(|t| t.job.is_none() && !t.gone) {
            Some(i) => i,
            None if self.threads.iter().filter(|t| !t.gone).count() < MAX_JOBS => {
                self.threads.push(spawn(wake, self.threads.len())?);
                self.threads.len() - 1
            }
            None => return Err(Refused("too_many")),
        };
        let Resolved { budget, namespace, handles } = resolved;
        let served = match &l.serve {
            Some(_) => Some(served()?),
            None => None,
        };
        let close_served = |served: &Option<(Endpoint, Endpoint)>| {
            if let Some((receive, send)) = served {
                let _ = redoubt_rt::handle::close(receive.handle());
                let _ = redoubt_rt::handle::close(send.handle());
            }
        };
        let exit = match Endpoint::create() {
            Ok(exit) => exit,
            Err(e) => {
                close_served(&served);
                return Err(Refused(kernel(e)));
            }
        };
        let mut builder = launch::Launch::new(
            STUB,
            &l.image,
            Budget::from_handle(budget),
            Endpoint::from_handle(exit.handle()),
        );
        for ((path, _), h) in l.namespace.iter().zip(namespace) {
            builder.namespace(path, h);
        }
        for ((n, _), h) in l.handles.iter().zip(handles) {
            builder.handle(n, h);
        }
        if let (Some(n), Some((receive, _))) = (&l.serve, &served) {
            builder.handle(n, receive.handle());
        }
        for arg in &l.args {
            builder.arg(arg);
        }
        if let Some(pages) = l.stack_pages {
            builder.stack_pages(usize::try_from(pages).unwrap_or(usize::MAX));
        }
        let heap_pages = match l.heap_pages.map(u32::try_from).transpose() {
            Ok(pages) => pages,
            Err(_) => {
                close_served(&served);
                let _ = redoubt_rt::handle::close(exit.handle());
                return Err(Refused("too_large"));
            }
        };
        if let Some(pages) = heap_pages {
            builder.heap_pages(pages);
        }
        let started = match builder.start() {
            Ok(started) => started,
            Err(failed) => {
                // The budget holds the process that never started; it is the caller's to destroy.
                close_served(&served);
                let _ = redoubt_rt::handle::close(exit.handle());
                return Err(name(failed.error));
            }
        };
        // The child holds its own copy of the receive right now: the VM keeps only the send right.
        let send = served.map(|(receive, send)| {
            let _ = redoubt_rt::handle::close(receive.handle());
            send
        });
        let process = started.process().handle();
        let thread = &mut self.threads[i];
        thread.watch.store(exit.handle().index(), Ordering::SeqCst);
        if thread.go.send(&[WATCH, 0, 0, 0], &[], None, HAND_US).is_err() {
            thread.gone = true;
            let _ = started.kill();
            let _ = redoubt_rt::handle::close(process);
            if let Some(send) = send {
                let _ = redoubt_rt::handle::close(send.handle());
            }
            return Err(Refused("protocol"));
        }
        let mut held = l.namespace.into_iter().chain(l.handles).map(|(_, o)| o).collect::<Vec<_>>();
        held.push(l.budget);
        thread.job = Some(Job { asker, job, exit, process, held });
        Ok(send)
    }

    /// Takes a job thread's notice: the job's end for its asker. Anything else is handed back.
    pub(crate) fn deliver(&mut self, delivery: Delivery) -> Result<(u64, Event), Delivery> {
        let i = delivery.caller.badge.wrapping_sub(BADGE) as usize;
        if delivery.words[0] != ENDED || i >= self.threads.len() {
            return Err(delivery);
        }
        let Some(job) = self.threads[i].job.take() else { return Err(delivery) };
        let _ = redoubt_rt::handle::close(job.process);
        let _ = redoubt_rt::handle::close(job.exit.handle());
        drop(job.held);
        let cause = match delivery.words[1] {
            c if c == Cause::Exited as u64 => "exited",
            c if c == Cause::Faulted as u64 => "faulted",
            c if c == Cause::Killed as u64 => "killed",
            _ => "ended",
        };
        Ok((job.asker, Event::Exit { job: job.job, cause, code: delivery.words[2] }))
    }
}

/// A new endpoint for a child to serve: its receive right, and the caller's send right to it.
fn served() -> Result<(Endpoint, Endpoint), Refused> {
    let receive = Endpoint::create().map_err(|e| Refused(kernel(e)))?;
    let badge = NonZeroU64::new(SERVED_BADGE).ok_or(Refused("protocol"))?;
    match receive.mint(badge, None) {
        Ok(send) => Ok((receive, send)),
        Err(e) => {
            let _ = redoubt_rt::handle::close(receive.handle());
            Err(Refused(kernel(e)))
        }
    }
}

/// Starts job thread `i`.
fn spawn(wake: &Endpoint, i: usize) -> Result<Thread, Refused> {
    let watch = Arc::new(AtomicU32::new(0));
    let shared = Arc::clone(&watch);
    let go =
        worker(wake, BADGE + i as u64, STACK_PAGES, move |receive, done| watch_jobs(receive, done, shared))?;
    Ok(Thread { watch, go, job: None, gone: false })
}

/// A job thread: for each job the VM names, waits for its exit notice and passes it on.
fn watch_jobs(receive: Endpoint, done: Endpoint, watch: Arc<AtomicU32>) {
    loop {
        match receive.receive(FOREVER, 0) {
            Ok(IpcEvent::Send(d)) if d.words[0] == WATCH => {}
            Ok(_) => continue,
            Err(_) => return,
        }
        let Some(exit) = Handle::new(watch.load(Ordering::SeqCst)) else { continue };
        let exit = Endpoint::from_handle(exit);
        let notice = loop {
            match exit.receive(FOREVER, 0) {
                Ok(IpcEvent::Exit(notice)) => break Some(notice),
                // Nothing else is ever sent there: a call is refused on drop, a send's handles closed.
                Ok(IpcEvent::Send(d)) => redoubt_rt::server::close_delivery(&d),
                Ok(_) => {}
                Err(_) => break None,
            }
        };
        let words = match notice {
            Some(n) => [ENDED, n.cause as u64, u64::from(n.code), 0],
            None => [ENDED, 0, 0, 0],
        };
        if done.send(&words, &[], None, FOREVER).is_err() {
            return;
        }
    }
}
