//! The reboot rule (servers/init.md, "Restarts and reboots"): a server already restarted
//! [`MOST`] times within the last [`WINDOW`] is not restarted again, and `init` reboots the
//! machine instead, because failing closed beats a server that cannot stay up.

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
}
