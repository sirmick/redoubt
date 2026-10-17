//! The cleanup queue (servers/steward.md, "Failure and restart"): what an object's end still owes
//! its servers once the scope is destroyed. The destroy is the cutoff ([R41](../../docs/servers/steward.md));
//! giving each connection back (`disconnect` by its id, `ended` for `sshd`'s channel console)
//! only frees the server's records, and a server that is busy or recovering would hold the
//! steward's loop for the call's whole bound while it serves nobody. So an end batch answers
//! after `DestroyBudget`, and each connection it gives back is a step here, run at the loop's
//! turn within the steward's own send bound, retried no sooner than [`RETRY_US`] after a step
//! the server did not take, and parked after [`TRIES`] attempts with a console line.
//!
//! A turn ([`Queue::run`]) stops at its first step the server did not take: the rest stay due,
//! so the loop receives once between two waits and a turn costs it one send bound at most
//! beside the steps that answer.
//!
//! A parked step has no timer: a server that never answers again costs the loop nothing while
//! its steps are parked. It runs again when the server shows it answers, which is a call of the
//! steward's to that server completed within its bound: a step there done ([`Queue::run`]
//! runs the server's parked steps in the same turn), or a mint there taken or refused
//! ([`Queue::wake`], the next turn); and before a mint the steward makes at a server with
//! parked steps ([`Queue::run_parked`]: the mint needs the record a step frees, and the first
//! timeout stops it, so the mint pays at most one bound). A server that restarted forgets its
//! parked steps with its records ([`Queue::forget`]).
//!
//! The queue's bound is the connections live sessions and leases hold: a connection enters once,
//! when its owner's end releases it (or `rebind` replaces it), and leaves done, or parked, which
//! is a queued step that has not left, so the queue never holds more than the connections of
//! the objects that ended, parked ones included. The bound is argued, not enforced: the queue
//! has no cap.

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
    /// The server the step is told at, by the name the steward reaches it under; none for a
    /// step nobody is told, which is done at its first run.
    server: Option<String>,
    /// Attempts made so far.
    tries: u8,
    /// When the next attempt may run.
    due: u64,
}

/// The steps not yet done, in the order their owners ended.
pub struct Queue<H> {
    steps: VecDeque<Step<H>>,
    /// The steps their servers did not take in [`TRIES`] attempts, waiting for their server's
    /// next answer, in the same order.
    parked: VecDeque<Step<H>>,
}

impl<H> Default for Queue<H> {
    fn default() -> Self { Queue { steps: VecDeque::new(), parked: VecDeque::new() } }
}

impl<H> Queue<H> {
    /// Queues `handle`, told at `server`, due at once.
    pub fn push(&mut self, handle: H, server: Option<String>, now: u64) {
        self.steps.push_back(Step { handle, server, tries: 0, due: now });
    }

    /// The steps waiting, parked ones included.
    pub fn len(&self) -> usize { self.steps.len() + self.parked.len() }

    pub fn is_empty(&self) -> bool { self.steps.is_empty() && self.parked.is_empty() }

    /// The steps parked.
    pub fn parked(&self) -> usize { self.parked.len() }

    /// When the earliest step is due, if any waits: a parked step has no time.
    pub fn due(&self) -> Option<u64> { self.steps.iter().map(|s| s.due).min() }

    /// `server` answered a call of the steward's: its parked steps are due at once.
    pub fn wake(&mut self, server: &str) {
        let mut kept = VecDeque::new();
        while let Some(mut step) = self.parked.pop_front() {
            if step.server.as_deref() == Some(server) {
                step.due = 0;
                self.steps.push_back(step);
            } else {
                kept.push_back(step);
            }
        }
        self.parked = kept;
    }

    /// `server` restarted: what its parked steps would have freed died with its old instance.
    pub fn forget(&mut self, server: &str) { self.parked.retain(|s| s.server.as_deref() != Some(server)); }

    /// Runs the steps due at `now` in order through `clean`, told whether this is the attempt
    /// whose timeout parks the step, up to and including the first that times out:
    /// `Err(Timeout)` keeps a step for [`RETRY_US`] later, up to [`TRIES`], then parks it, and
    /// ends the turn, the steps behind it still due; any other outcome ends the step and wakes
    /// its server's parked steps, which run in this turn. Returns when the earliest remaining
    /// step is due: `now` when the turn stopped short.
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
            step.tries = step.tries.saturating_add(1);
            let parks = step.tries == TRIES;
            if matches!(clean(step.handle, parks), Err(Error::Timeout)) {
                if step.tries < TRIES {
                    step.due = now.saturating_add(RETRY_US);
                    kept.push_back(step);
                } else {
                    self.parked.push_back(step);
                }
                break;
            }
            if let Some(server) = &step.server {
                self.wake(server);
            }
        }
        kept.append(&mut self.steps);
        self.steps = kept;
        self.due()
    }

    /// Runs the steps parked at `server` in order through `clean`, before a mint there: a step
    /// done leaves; the first that times out stays parked with the rest behind it, so the mint
    /// waits one bound at most.
    pub fn run_parked(&mut self, server: &str, mut clean: impl FnMut(H) -> Result<(), Error>)
    where
        H: Copy,
    {
        let mut kept = VecDeque::new();
        let mut stopped = false;
        while let Some(mut step) = self.parked.pop_front() {
            if stopped || step.server.as_deref() != Some(server) {
                kept.push_back(step);
                continue;
            }
            step.tries = step.tries.saturating_add(1);
            if matches!(clean(step.handle), Err(Error::Timeout)) {
                kept.push_back(step);
                stopped = true;
            }
        }
        self.parked = kept;
    }
}
