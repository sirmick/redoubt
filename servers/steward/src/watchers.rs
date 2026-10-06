//! The steward's exit watchers (servers/steward.md, "Authentication and sessions"): a thread
//! waits on a session's exit endpoint for its one notice and reports it. A thread's stack is never
//! given back (the runtime keeps it), so threads are reused: a watcher that has reported waits for
//! the next session, and a new one starts only when every watcher is watching. There are never
//! more watchers than sessions alive at once, however many come and go.

/// The watchers started and the sessions they are watching.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Watchers {
    started: usize,
    watching: usize,
}

impl Watchers {
    /// A session is launched: whether a new watcher must start for it, since every one is
    /// watching.
    pub fn watch(&mut self) -> bool {
        self.watching += 1;
        if self.watching > self.started {
            self.started += 1;
            return true;
        }
        false
    }

    /// A launch whose watcher could not start, or whose session never started: it is watched by
    /// nobody.
    pub fn unwatch(&mut self, started_one: bool) {
        self.watching = self.watching.saturating_sub(1);
        if started_one {
            self.started = self.started.saturating_sub(1);
        }
    }

    /// A watcher reported its session's end and waits for the next.
    pub fn reported(&mut self) { self.watching = self.watching.saturating_sub(1); }

    /// The watchers started.
    pub fn started(&self) -> usize { self.started }
}

#[cfg(test)]
mod tests {
    use super::Watchers;

    /// A login and logout a thousand times over, at most two sessions at once: two watchers, not
    /// a thousand.
    #[test]
    fn sessions_coming_and_going_reuse_the_watchers() {
        let mut w = Watchers::default();
        assert!(w.watch(), "the first session starts a watcher");
        for _ in 0..1000 {
            w.watch();
            w.reported();
        }
        assert_eq!(w.started(), 2);
        w.reported();
        assert!(!w.watch(), "an idle watcher takes the next session");
        assert_eq!(w.started(), 2);
    }

    /// A watcher that could not start is not counted, and the next launch tries again.
    #[test]
    fn a_watcher_that_did_not_start_is_not_counted() {
        let mut w = Watchers::default();
        let new = w.watch();
        w.unwatch(new);
        assert_eq!(w, Watchers::default());
        assert!(w.watch());
    }
}
