//! Every interleaving of a release and a wait on one lock, each shared access of [`Waits`] one
//! step. Hart 0 holds the lock and releases it; hart 1 holds the next ticket and waits for its
//! turn; hart 2 holds the ticket after and is already halted, marked. A run is one order of harts
//! 0 and 1's steps, and the runs enumerate every order, depth first, by replaying a prefix of
//! choices. Every access is sequentially consistent, so these orders are every execution the two
//! sides have.
//!
//! A halted hart runs again once a wake is pending for it, as an interrupt stays pending for a `wfi`
//! (the kernel's halt returns at once then, and clears it). In the second test a halt may also end
//! once with no wake, as the kernel's does for a shootdown's interrupt or one that was already
//! pending, and the hart marks itself again. Checked in every run: hart 1 is never left halted
//! with nothing to wake it (a lost wake-up), and the release wakes neither hart 2, whose turn has
//! not come, nor any hart that then finds its turn has not come.

extern crate std;

use core::sync::atomic::{AtomicU32, Ordering::SeqCst};
use std::string::String;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::vec::Vec;
use std::{format, thread, vec};

use super::Waits;

/// Harts 0 to 2, hart `h` holding ticket `h`.
const HARTS: usize = 3;
/// The harts that run: hart 2 stays halted.
const RUN: usize = 2;
/// How many orders the steps have: with no halt ended without a wake, and with one.
const ORDERS: u32 = 597;
const ORDERS_SPURIOUS: u32 = 5_961;

#[derive(Clone, Copy, PartialEq)]
enum State {
    Running,
    AtStep,
    Halted,
    Done,
}

struct World {
    states: Vec<State>,
    /// A wake sent and not yet taken by a halt.
    pending: Vec<bool>,
    /// The hart the scheduler let run its next step.
    granted: Option<usize>,
    /// Halts that may still end with no wake pending.
    spurious: u32,
    /// A run found stuck: every hart still parked unwinds.
    abort: bool,
    /// What a run broke, in words.
    broken: Vec<String>,
}

struct Sched {
    world: Mutex<World>,
    turn: Condvar,
}

impl Sched {
    /// The world, whether or not an abandoned run's hart unwound holding it.
    fn world(&self) -> MutexGuard<'_, World> { self.world.lock().unwrap_or_else(|e| e.into_inner()) }

    /// Park hart `me` before its next step (or halted), until the scheduler picks it; for a halt,
    /// whether a wake ended it.
    fn park(&self, me: usize, state: State) -> bool {
        let mut w = self.world();
        w.states[me] = state;
        self.turn.notify_all();
        while w.granted != Some(me) {
            assert!(!w.abort, "run abandoned");
            w = self.turn.wait(w).unwrap_or_else(|e| e.into_inner());
        }
        w.granted = None;
        w.states[me] = State::Running;
        let woken = state == State::Halted && core::mem::take(&mut w.pending[me]);
        if state == State::Halted && !woken {
            w.spurious -= 1;
        }
        self.turn.notify_all();
        woken
    }

    fn wake(&self, hart: usize) { self.world().pending[hart] = true }

    fn broke(&self, what: String) { self.world().broken.push(what) }

    fn done(&self, me: usize) {
        let mut w = self.world();
        w.states[me] = State::Done;
        self.turn.notify_all();
    }

    /// Run the harts to the end, picking among those that can run with `choose`; `Err` if they
    /// stop with a hart halted, its turn come, and no wake pending for it.
    fn run(&self, serving: &AtomicU32, mut choose: impl FnMut(usize) -> usize) -> Result<(), String> {
        let mut w = self.world();
        loop {
            while w.granted.is_some() || w.states.contains(&State::Running) {
                w = self.turn.wait(w).unwrap_or_else(|e| e.into_inner());
            }
            let ready: Vec<usize> = (0..HARTS)
                .filter(|&h| {
                    w.states[h] == State::AtStep
                        || (w.states[h] == State::Halted && (w.pending[h] || (h == 1 && w.spurious > 0)))
                })
                .collect();
            if ready.is_empty() {
                let served = serving.load(SeqCst) as usize;
                let lost: Vec<usize> = (0..=served).filter(|&h| w.states[h] == State::Halted).collect();
                if lost.is_empty() {
                    return Ok(());
                }
                w.abort = true;
                self.turn.notify_all();
                return Err(format!("harts {lost:?} halted, their turn come, with no wake to come"));
            }
            w.granted = Some(ready[choose(ready.len())]);
            self.turn.notify_all();
        }
    }
}

/// Hart `me` with ticket `me`: wait for its turn (hart 0 holds the lock already), and hart 0
/// releases it.
fn hart(me: usize, waits: &Waits<HARTS>, serving: &AtomicU32, sched: &Sched) {
    let step = || {
        sched.park(me, State::AtStep);
    };
    let ticket = me as u32;
    loop {
        step();
        if serving.load(SeqCst) == ticket {
            break;
        }
        let mut woken = false;
        waits.wait_stepped(me, ticket, serving, || woken = sched.park(me, State::Halted), &step);
        if woken && serving.load(SeqCst) != ticket {
            sched.broke(format!("hart {me} woken before its turn"));
        }
    }
    if me == 0 {
        let wake = waits.release_stepped(serving, &step);
        if let Some(hart) = wake.filter(|&hart| hart != 1) {
            sched.broke(format!("the release to ticket 1 woke hart {hart}"));
        }
        step();
        if let Some(hart) = wake {
            sched.wake(hart);
        }
    }
    sched.done(me);
}

/// One run, its choices following `prefix` and then the first ready hart, hart 1's halt ending
/// with no wake up to `spurious` times; the choices made, each with how many it had.
fn run_once(prefix: &[usize], spurious: u32) -> (Vec<(usize, usize)>, Result<(), String>) {
    let waits = Arc::new(Waits::<HARTS>::new());
    // Hart 2 waits for ticket 2, halted.
    waits.tickets[2].store(2, SeqCst);
    waits.halted.fetch_or(1 << 2, SeqCst);
    let serving = Arc::new(AtomicU32::new(0));
    let mut states = vec![State::Running; HARTS];
    states[2] = State::Halted;
    let sched = Arc::new(Sched {
        world: Mutex::new(World {
            states,
            pending: vec![false; HARTS],
            granted: None,
            spurious,
            abort: false,
            broken: Vec::new(),
        }),
        turn: Condvar::new(),
    });
    let threads: Vec<_> = (0..RUN)
        .map(|me| {
            let (waits, serving, sched) = (waits.clone(), serving.clone(), sched.clone());
            thread::spawn(move || hart(me, &waits, &serving, &sched))
        })
        .collect();
    let mut made = Vec::new();
    let mut outcome = sched.run(&serving, |ready| {
        let choice = prefix.get(made.len()).copied().unwrap_or(0);
        made.push((choice, ready));
        choice
    });
    for thread in threads {
        // A stuck run's parked harts unwind ("run abandoned"); the run's outcome says why.
        let _ = thread.join();
    }
    let broken = &sched.world().broken;
    if outcome.is_ok() && !broken.is_empty() {
        outcome = Err(broken.join("; "));
    }
    (made, outcome)
}

/// Every order, each run's halts ending with no wake up to `spurious` times; how many there were.
fn every_order(spurious: u32) -> u32 {
    let mut prefix = Vec::new();
    let mut runs = 0u32;
    loop {
        let (made, outcome) = run_once(&prefix, spurious);
        runs += 1;
        if let Err(why) = outcome {
            panic!("run {runs}, choices {:?}: {why}", made.iter().map(|c| c.0).collect::<Vec<_>>());
        }
        // The next order: the last choice that had another option, taken one further.
        let Some(at) = made.iter().rposition(|&(choice, ready)| choice + 1 < ready) else { break };
        prefix = made[..at].iter().map(|c| c.0).collect();
        prefix.push(made[at].0 + 1);
    }
    runs
}

#[test]
fn no_hart_halts_past_its_turn_and_a_release_wakes_only_the_hart_it_serves() {
    // Every order of hart 0's 6 steps against hart 1's 6 or 7, its wake's turn included: a
    // scheduler that stopped exploring early, or a step added to either side, shows here.
    assert_eq!(every_order(0), ORDERS, "orders explored");
}

#[test]
fn a_halt_ended_with_no_wake_marks_again_and_is_still_woken() {
    // The same, and hart 1's halt may end once with nothing pending, then mark and re-read again:
    // ten times the orders, from the second turn of its wait.
    assert_eq!(every_order(1), ORDERS_SPURIOUS, "orders explored");
}
