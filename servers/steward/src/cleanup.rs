//! The cleanup queue (servers/steward.md, "Failure and restart"): what an object's end still owes
//! its servers once the scope is destroyed. The destroy is the cutoff ([R41](../../docs/servers/steward.md));
//! giving each connection back (`disconnect` by its id, `ended` for `sshd`'s channel console)
//! only frees the server's records, and a server that is busy or recovering would hold the
//! steward's loop for the call's whole bound while it serves nobody. So an end batch answers
//! after `DestroyBudget`, and each connection it gives back is a step here, run at the loop's
//! turn within the steward's own send bound, retried no sooner than [`RETRY_US`] after a step
//! the server did not take, and dropped after [`TRIES`] attempts with a console line.
//!
//! A turn ([`Queue::run`]) stops at its first step the server did not take: the rest stay due,
//! so the loop receives once between two waits and a turn costs it one send bound at most
//! beside the steps that answer.
//!
//! The queue's bound is the connections live sessions and leases hold: a connection enters once,
//! when its owner's end releases it (or `rebind` replaces it), and leaves done or dropped within
//! the retries' span, so the queue never holds more than the connections of the objects that
//! ended in that span. The bound is argued, not enforced: the queue has no cap.

use alloc::collections::VecDeque;

use redoubt_rt::abi::Error;

/// Attempts at one step before it is dropped. With [`RETRY_US`] between them the four span at
/// least 3.4 s: past `init`'s bound on a restarted server's first answer (`reconnect::SERVER_TIMEOUT`,
/// 3 s), so a server that has not served by then is lost to `init` too, and what the dropped
/// step would have freed is freed by that server's own restart.
pub const TRIES: u8 = 4;
/// The least time between two attempts at one step (µs): a server recovering from a restart
/// takes its log's replay, measured at 589 to 953 ms on the image's data volume.
pub const RETRY_US: u64 = 1_000_000;

/// One connection to give back, by the handle the kernel holds it under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step<H> {
    handle: H,
    /// Attempts made so far.
    tries: u8,
    /// When the next attempt may run.
    due: u64,
}

/// The steps not yet done, in the order their owners ended.
pub struct Queue<H> {
    steps: VecDeque<Step<H>>,
}

impl<H> Default for Queue<H> {
    fn default() -> Self { Queue { steps: VecDeque::new() } }
}

impl<H> Queue<H> {
    /// Queues `handle`, due at once.
    pub fn push(&mut self, handle: H, now: u64) { self.steps.push_back(Step { handle, tries: 0, due: now }); }

    /// The steps waiting.
    pub fn len(&self) -> usize { self.steps.len() }

    pub fn is_empty(&self) -> bool { self.steps.is_empty() }

    /// When the earliest step is due, if any waits.
    pub fn due(&self) -> Option<u64> { self.steps.iter().map(|s| s.due).min() }

    /// Runs the steps due at `now` in order through `clean`, told whether this is its last
    /// attempt, up to and including the first that times out: `Err(Timeout)` keeps a step for
    /// [`RETRY_US`] later, up to [`TRIES`], and ends the turn, the steps behind it still due;
    /// any other outcome ends the step. Returns when the earliest remaining step is due: `now`
    /// when the turn stopped short.
    pub fn run(&mut self, now: u64, mut clean: impl FnMut(H, bool) -> Result<(), Error>) -> Option<u64>
    where
        H: Copy,
    {
        let mut kept = VecDeque::new();
        while let Some(mut step) = self.steps.pop_front() {
            if step.due > now {
                kept.push_back(step);
                continue;
            }
            step.tries += 1;
            let last = step.tries >= TRIES;
            let timed_out = matches!(clean(step.handle, last), Err(Error::Timeout));
            if timed_out && !last {
                step.due = now.saturating_add(RETRY_US);
                kept.push_back(step);
            }
            if timed_out {
                break;
            }
        }
        kept.append(&mut self.steps);
        self.steps = kept;
        self.due()
    }
}
