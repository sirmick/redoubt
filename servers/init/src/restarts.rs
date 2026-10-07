//! The reboot rule (servers/init.md, "Restarts and reboots"): a server already restarted
//! [`MOST`] times within the last [`WINDOW`] is not restarted again, and `init` reboots the
//! machine instead, because failing closed beats a server that cannot stay up. A dead steward's
//! `users` is emptied before it starts again ([`empty`]).

/// The restarts of one server that may fall within [`WINDOW`].
pub const MOST: usize = 5;
/// The window the restarts are counted in, in µs of `time_now`: 60 seconds.
pub const WINDOW: u64 = 60_000_000;

/// One server's last [`MOST`] restart times, none until it has restarted that often.
#[derive(Clone, Copy, Debug, Default)]
pub struct Restarts {
    times: [Option<u64>; MOST],
    /// Where the next restart goes, over the oldest.
    next: usize,
}

impl Restarts {
    /// On the server's exit at `now`: whether it may restart. A restart that may is counted; one
    /// that may not leaves the count as it was.
    pub fn restart(&mut self, now: u64) -> bool {
        let recent = self.times.iter().flatten().filter(|&&t| now.saturating_sub(t) < WINDOW).count();
        if recent >= MOST {
            return false;
        }
        self.times[self.next] = Some(now);
        self.next = (self.next + 1) % MOST;
        true
    }
}

/// Empties a budget one child at a time: `reap` is `budget_reap` on it, which destroys one child
/// and returns how many are left (kernel/budgets.md, R10). `occupied` is whether the budget holds
/// anything: a reap of a budget with no children also returns 0, having reaped nothing, so the
/// count starts from what the budget's usage says. Returns how many children were reaped, or the
/// first error. Nothing carves under the budget meanwhile (its other holder, the steward, is
/// dead), so each reap leaves one fewer and the loop ends.
pub fn empty<E>(occupied: bool, mut reap: impl FnMut() -> Result<u32, E>) -> Result<u32, E> {
    if !occupied {
        return Ok(0);
    }
    let mut reaped = 0;
    loop {
        reaped += 1;
        if reap()? == 0 {
            return Ok(reaped);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn the_fifth_restart_goes_ahead_and_the_sixth_exit_reboots() {
        let mut r = Restarts::default();
        for n in 0..MOST as u64 {
            assert!(r.restart(n * S), "restart {}", n + 1);
        }
        assert!(!r.restart(5 * S));
        // Refused, it is not counted: the machine reboots, and nothing asks again.
        assert!(!r.restart(5 * S + 1));
    }

    #[test]
    fn a_restart_older_than_the_window_is_dropped_from_the_count() {
        let mut r = Restarts::default();
        for n in 0..MOST as u64 {
            assert!(r.restart(n * S));
        }
        // The first, at 0, is exactly 60 s old at 60 s: out of the window, so four remain.
        assert!(r.restart(WINDOW));
        // Now 1 to 4 s and 60 s: five within the window at 60.5 s.
        assert!(!r.restart(WINDOW + S / 2));
        // At 61 s the one at 1 s has left the window too.
        assert!(r.restart(WINDOW + S));
    }

    #[test]
    fn restarts_spread_wider_than_the_window_never_reboot() {
        let mut r = Restarts::default();
        for n in 0..100u64 {
            assert!(r.restart(n * (WINDOW / MOST as u64)), "restart {}", n + 1);
        }
    }

    #[test]
    fn a_clock_that_reads_earlier_counts_the_restart_as_recent() {
        let mut r = Restarts::default();
        for _ in 0..MOST {
            assert!(r.restart(10 * S));
        }
        assert!(!r.restart(9 * S));
    }

    #[test]
    fn emptying_reaps_until_none_are_left_and_counts_each() {
        let mut left = 3u32;
        let mut calls = 0;
        let reaped = empty::<()>(true, || {
            calls += 1;
            left -= 1;
            Ok(left)
        });
        assert_eq!((reaped, calls, left), (Ok(3), 3, 0));
    }

    #[test]
    fn an_empty_budget_is_not_reaped() {
        assert_eq!(empty::<()>(false, || panic!("reaped an empty budget")), Ok(0));
    }

    #[test]
    fn a_refused_reap_stops_the_loop_with_its_error() {
        let mut calls = 0;
        let r = empty(true, || {
            calls += 1;
            if calls == 2 { Err("refused") } else { Ok(5) }
        });
        assert_eq!((r, calls), (Err("refused"), 2));
    }
}
