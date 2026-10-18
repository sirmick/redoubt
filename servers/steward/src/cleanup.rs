//! The cleanup queue (servers/steward.md, "Failure and restart"): what an object's end still owes
//! its servers once the scope is destroyed. The destroy is the cutoff ([R41](../../docs/servers/steward.md));
//! giving each connection back (`disconnect` by its id, `ended` for `sshd`'s channel console)
//! only frees the server's records, and a server that is busy or recovering would hold whoever
//! makes the call for the call's whole bound while it serves nobody. So an end batch answers
//! after `DestroyBudget`, each connection it gives back is a step here, and the steps are run by
//! a cleanup thread of the steward's, never by its loop: the loop hands a step over
//! ([`Queue::push`]), the thread takes the steps due in order ([`Queue::take`]), makes each call
//! within the client library's own call bound, and settles the outcome ([`Queue::settle`]): a
//! step the server did not take (`Timeout`) is retried no sooner than [`RETRY_US`] after, and
//! parked after [`TRIES`] attempts; any other outcome ends it. The thread reports each step's
//! fate to the loop, which says the parked step's console line.
//!
//! A parked step has no timer: a server that never answers again costs nobody anything while its
//! steps are parked. It runs again when the server shows it answers, which is a call of the
//! steward's to that server completed within its bound: a step there done ([`Queue::settle`]
//! wakes the server's parked steps, which the thread takes next), or a mint there taken or
//! refused ([`Queue::wake`]); and the loop wakes a server's parked steps before a mint it makes
//! there, since the mint needs the record a step frees, though the mint itself waits for
//! nothing: refused all the same, it fails its step. A server that restarted forgets its steps
//! with its records ([`Queue::forget`]).
//!
//! The queue's bound is the connections live sessions and leases hold: a connection enters once,
//! when its owner's end releases it (or `rebind` replaces it), and leaves done, or parked, which
//! is a queued step that has not left, so the queue never holds more than the connections of
//! the objects that ended, parked ones included. The bound is argued, not enforced: the queue
//! has no cap. The queue is shared by the two threads under a lock the machine owns; neither
//! holds it across a call.

use alloc::collections::VecDeque;
use alloc::string::String;

use redoubt_rt::abi::Error;

/// Attempts at one step before it is parked. With [`RETRY_US`] between them the four span at
/// least 3.4 s: past `init`'s bound on a restarted server's first answer (`reconnect::SERVER_TIMEOUT`,
/// 3 s), so a server that has not served by then is lost to `init` too, and what the parked
/// step would have freed is freed by that server's own restart; a server that is busy, not
/// lost, is answered by persistence, not by more attempts on a timer.
pub const TRIES: u8 = 4;
/// The least time between two attempts at one step (µs): a server recovering from a restart
/// takes its log's replay, measured at 589 to 953 ms on the image's data volume.
pub const RETRY_US: u64 = 1_000_000;

/// One connection to give back, by the handle the kernel holds it under.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Step<H> {
    handle: H,
    /// The server the step is told at, by the name the steward reaches it under.
    server: String,
    /// Attempts made so far.
    tries: u8,
    /// When the next attempt may run.
    due: u64,
}

/// How a step's attempt ended, as the thread reports it to the loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    /// The server took the call: the step is done.
    Done,
    /// The server did not take it in time; the step is due again [`RETRY_US`] later.
    Retry,
    /// The server did not take it in [`TRIES`] attempts: the step waits for the server's next
    /// answer, with the attempts made.
    Parked(u8),
    /// The call failed otherwise (the server refused it, or is dead): the step is done, there is
    /// nothing more to tell.
    Failed(Error),
}

/// The steps not yet done, in the order their owners ended.
pub struct Queue<H> {
    steps: VecDeque<Step<H>>,
    /// The steps their servers did not take in [`TRIES`] attempts, waiting for their server's
    /// next answer, in the same order.
    parked: VecDeque<Step<H>>,
    /// The step the thread is making the call of.
    running: Option<Step<H>>,
}

impl<H> Default for Queue<H> {
    fn default() -> Self { Queue { steps: VecDeque::new(), parked: VecDeque::new(), running: None } }
}

impl<H> Queue<H> {
    /// Queues `handle`, told at `server`, due at once.
    pub fn push(&mut self, handle: H, server: String, now: u64) {
        self.steps.push_back(Step { handle, server, tries: 0, due: now });
    }

    /// The steps waiting, parked ones and the one running included.
    pub fn len(&self) -> usize { self.steps.len() + self.parked.len() + usize::from(self.running.is_some()) }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// The steps parked.
    pub fn parked(&self) -> usize { self.parked.len() }

    /// The steps parked at `server`.
    pub fn parked_at(&self, server: &str) -> usize {
        self.parked.iter().filter(|s| s.server == server).count()
    }

    /// When the earliest step is due, if any waits: a parked step has no time.
    pub fn due(&self) -> Option<u64> { self.steps.iter().map(|s| s.due).min() }

    /// `server` answered a call of the steward's: its parked steps are due at once. Whether any
    /// was.
    pub fn wake(&mut self, server: &str) -> bool {
        let mut kept = VecDeque::new();
        let mut woke = false;
        while let Some(mut step) = self.parked.pop_front() {
            if step.server == server {
                step.due = 0;
                self.steps.push_back(step);
                woke = true;
            } else {
                kept.push_back(step);
            }
        }
        self.parked = kept;
        woke
    }

    /// `server` restarted: what its steps would have freed died with its old instance, the
    /// parked ones' and the due ones' alike. A step whose call is out stays out: its outcome
    /// settles it.
    pub fn forget(&mut self, server: &str) {
        self.parked.retain(|s| s.server != server);
        self.steps.retain(|s| s.server != server);
    }

    /// Takes the first step due at `now`, if any, as the thread's running one, counting the
    /// attempt: the handle and the attempt's number. None while one is running.
    pub fn take(&mut self, now: u64) -> Option<(H, u8)>
    where
        H: Copy,
    {
        if self.running.is_some() {
            return None;
        }
        let at = self.steps.iter().position(|s| s.due <= now)?;
        let mut step = self.steps.remove(at)?;
        step.tries = step.tries.saturating_add(1);
        let taken = (step.handle, step.tries);
        self.running = Some(step);
        Some(taken)
    }

    /// Settles the running step's call: `Err(Timeout)` keeps it for [`RETRY_US`] later, up to
    /// [`TRIES`], then parks it; any other outcome ends it and wakes its server's parked steps.
    /// Returns the step and its fate; None with nothing running.
    pub fn settle(&mut self, told: Result<(), Error>, now: u64) -> Option<(H, Fate)>
    where
        H: Copy,
    {
        let mut step = self.running.take()?;
        let handle = step.handle;
        let fate = match told {
            Err(Error::Timeout) if step.tries < TRIES => {
                step.due = now.saturating_add(RETRY_US);
                self.steps.push_front(step);
                Fate::Retry
            }
            Err(Error::Timeout) => {
                let tries = step.tries;
                self.parked.push_back(step);
                Fate::Parked(tries)
            }
            Ok(()) => {
                self.wake(&step.server);
                Fate::Done
            }
            Err(e) => {
                self.wake(&step.server);
                Fate::Failed(e)
            }
        };
        Some((handle, fate))
    }
}
