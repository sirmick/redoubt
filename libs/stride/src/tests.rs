//! Unit tests of the rules on their own. `tests/differential.rs` checks them against the model.

extern crate std;

use std::collections::BTreeMap;
use std::vec;
use std::vec::Vec;

use super::*;

/// A small deterministic generator (xorshift), so every case is reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 { lo + self.next() % (hi - lo + 1) }
}

#[test]
fn a_split_charge_equals_the_whole() {
    // Exhaustive over small weights and splits: the remainder makes charging exact.
    for w in 1..=64u64 {
        for total in 0..=200u64 {
            let mut whole = State::default();
            charge(&mut whole, w, total);
            for split in 0..=total {
                let mut parts = State::default();
                charge(&mut parts, w, split);
                charge(&mut parts, w, total - split);
                assert_eq!(
                    (parts.pass, parts.rem),
                    (whole.pass, whole.rem),
                    "w={w} total={total} split={split}"
                );
            }
        }
    }
    // Random large weights and runtimes, in many pieces.
    let mut rng = Rng(0x5eed);
    for _ in 0..10_000 {
        let w = rng.range(1, u64::from(u32::MAX));
        let pieces: Vec<u64> = (0..rng.range(1, 20)).map(|_| rng.range(0, 1 << 30)).collect();
        let mut whole = State::default();
        charge(&mut whole, w, pieces.iter().sum());
        let mut parts = State::default();
        for p in &pieces {
            charge(&mut parts, w, *p);
        }
        assert_eq!((parts.pass, parts.rem), (whole.pass, whole.rem), "w={w}");
        assert!(parts.rem < w);
    }
}

#[test]
fn every_charge_counts_at_any_weight() {
    // A weight far above STRIDE: one-tick runs are charged nothing on their own, but the
    // remainder accumulates them exactly.
    let w = 1u64 << 30;
    let mut s = State::default();
    for _ in 0..(w / STRIDE) {
        charge(&mut s, w, 1);
    }
    assert_eq!((s.pass, s.rem), (1, 0));
    // The cap keeps the arithmetic in range for any runtime, on either width.
    let mut s = State { rem: u64::from(u32::MAX) - 1, ..State::default() };
    charge(&mut s, u64::from(u32::MAX), u64::MAX);
    assert!(s.rem < u64::from(u32::MAX));
}

#[test]
fn a_carve_and_its_return_leave_the_state() {
    let mut rng = Rng(7);
    for _ in 0..100_000 {
        let f = u128::from(rng.range(0, 1 << 40));
        let w = rng.range(2, u64::from(u32::MAX));
        let kept = rng.range(1, w - 1);
        let s = State {
            pass: f + u128::from(rng.range(0, 1 << 30)),
            rem: rng.range(0, w - 1),
            ..State::default()
        };
        // What it owes is kept exactly at the smaller weight...
        let mut carved = s;
        rescale(&mut carved, w, kept, f);
        assert!(carved.rem < kept);
        let owed = |x: &State, w: u64| (x.pass - f) * u128::from(w) + u128::from(x.rem);
        assert_eq!(owed(&carved, kept), owed(&s, w));
        // ...and the carve's return with no run between restores the state.
        let mut back = carved;
        rescale(&mut back, kept, w, f);
        assert_eq!(back, s);
    }
    // A carve raises the lead by the ratio of the weights: lead 20 at 10 is lead 200 at 1.
    let mut s = State { pass: 120, rem: 3, ..State::default() };
    rescale(&mut s, 10, 1, 100);
    assert_eq!((s.pass, s.rem), (303, 0));
    // Below the floor only the remainder is owed; weight 0 converts nothing.
    let mut low = State { pass: 90, rem: 7, ..State::default() };
    rescale(&mut low, 10, 5, 100);
    assert_eq!((low.pass, low.rem), (101, 2));
    // Through weight 0 what it owes is carried exactly: a carve of everything (10 to 0) and the
    // return of only part of it (0 to 5) is the carve 10 to 5.
    let s = State { pass: 120, rem: 3, ..State::default() };
    let mut direct = s;
    rescale(&mut direct, 10, 5, 100);
    let mut through = s;
    rescale(&mut through, 10, 0, 100);
    rescale(&mut through, 0, 5, 100);
    assert_eq!(through, direct);
    let mut back = through;
    rescale(&mut back, 5, 0, 100);
    rescale(&mut back, 0, 10, 100);
    assert_eq!(back, s);
}

#[test]
fn create_then_destroy_without_a_run_moves_nothing() {
    let mut rng = Rng(11);
    for _ in 0..100_000 {
        let f = u128::from(rng.range(0, 1 << 40));
        let parent_pass = f + u128::from(rng.range(0, 1 << 30));
        let wp = rng.range(2, u64::from(u32::MAX));
        let mut parent = State { pass: parent_pass, rem: rng.range(0, wp - 1), ..State::default() };
        let e = entry(f, Some(parent.pass));
        let child = State { pass: e, entry: e, ..State::default() };
        let before = parent;
        lift(&mut parent, &child, rng.range(1, wp - 1), wp, f);
        assert_eq!(parent, before);
    }
}

#[test]
fn a_churned_child_adds_to_a_leading_parent() {
    // P (100, spinning, lead S/99 after its own slice at 99) destroys C (1), which ran a slice
    // (work S). The additive rule charges both at P's restored weight: f + S/100 + S/100.
    let s = 10_000 * STRIDE; // one slice of work
    let f: u128 = 1 << 40;
    let mut p = State { pass: f + u128::from(s / 99), rem: s % 99, ..State::default() };
    let c = State { pass: f + u128::from(s), entry: f, ..State::default() };
    // P's lead was run at weight 99; its carve returning restates it at 100, exactly.
    rescale(&mut p, 99, 100, f);
    lift(&mut p, &c, 1, 100, f);
    assert_eq!((p.pass, p.rem), (f + 2 * u128::from(s / 100), 0));
    // A parent below the floor starts from the floor: it cannot bank what it did not run.
    let mut low = State { pass: f - 5, rem: 3, ..State::default() };
    lift(&mut low, &State { pass: f, entry: f, ..State::default() }, 1, 100, f);
    assert_eq!((low.pass, low.rem), (f, 0));
}

#[test]
fn the_inherited_wait_is_not_counted_again() {
    // The child entered at the parent's lead; only what it ran after entry moves up.
    let f: u128 = 1000;
    let lead: u128 = 500;
    let mut p = State { pass: f + lead, ..State::default() };
    let e = entry(f, Some(p.pass));
    let c = State { pass: e + 7, entry: e, ..State::default() };
    lift(&mut p, &c, 10, 10, f);
    assert_eq!(p.pass, f + lead + 7);
}

/// Budgets in a map, for the queue tests.
#[derive(Default)]
struct Map(BTreeMap<u64, (State, u64)>);

impl Budgets<u64> for Map {
    fn state(&self, b: u64) -> State { self.0[&b].0 }

    fn set_state(&mut self, b: u64, s: State) { self.0.get_mut(&b).unwrap().0 = s; }

    fn id(&self, b: u64) -> u64 { b }

    fn weight(&self, b: u64) -> u64 { self.0[&b].1 }
}

fn map(ids: &[u64]) -> Map {
    let mut m = Map::default();
    for id in ids {
        m.0.insert(*id, (State::default(), 10));
    }
    m
}

/// A reconcile that visits every queued budget and every one in `runnable`, the budgets with a
/// runnable thread: what one that visits only the marked budgets must agree with.
fn reconcile<S: Budgets<u64>, const N: usize>(
    q: &mut Queue<u64, N>,
    bs: &mut S,
    running: Option<u64>,
    runnable: &[u64],
) {
    let lost: Vec<u64> = q.queued().collect();
    let mut gained = runnable.to_vec();
    q.reconcile(bs, |b| running == Some(b), &lost, &mut gained, |_, b| runnable.contains(&b));
}

#[test]
fn ranks_follow_all_four_clauses() {
    let mut bs = map(&[1, 2, 3, 4, 5]);
    let mut q: Queue<u64, 8> = Queue::new();
    // Clause 3: wakes in one reconcile run lowest id first.
    reconcile(&mut q, &mut bs, None, &[3, 1, 2]);
    assert_eq!(q.pick(|_| false), Some(1));
    // Budget 1 runs a slice and is requeued; its pass is now higher, so 2 is next.
    q.fold(&mut bs, 1, 100);
    q.deschedule(&mut bs, 1, true);
    assert_eq!(q.pick(|_| false), Some(2));
    // Clause 2: a later reconcile's wake at the same pass (the floor) goes ahead of earlier ones.
    reconcile(&mut q, &mut bs, None, &[1, 2, 3, 4]);
    assert_eq!(q.pick(|_| false), Some(4));
    // Clause 1 and 4: requeued budgets at an equal pass go behind wakers, in FIFO order.
    q.fold(&mut bs, 4, 0);
    q.deschedule(&mut bs, 4, true);
    q.fold(&mut bs, 2, 0);
    q.deschedule(&mut bs, 2, true);
    q.fold(&mut bs, 3, 0);
    q.deschedule(&mut bs, 3, true);
    // 4, 2, 3 are requeued (in that order) at the floor; a new waker 5 at the floor goes first.
    reconcile(&mut q, &mut bs, None, &[1, 2, 3, 4, 5]);
    let order: Vec<u64> = (0..4)
        .map(|_| {
            let b = q.pick(|_| false).unwrap();
            q.fold(&mut bs, b, 1000);
            q.deschedule(&mut bs, b, true);
            b
        })
        .collect();
    assert_eq!(order, [5, 4, 2, 3]);
}

/// A [`Map`] that records the budgets that left the queue, in order.
#[derive(Default)]
struct Leaving(Map, Vec<u64>);

impl Budgets<u64> for Leaving {
    fn state(&self, b: u64) -> State { self.0.state(b) }

    fn set_state(&mut self, b: u64, s: State) { self.0.set_state(b, s) }

    fn id(&self, b: u64) -> u64 { b }

    fn weight(&self, b: u64) -> u64 { self.0.weight(b) }

    fn left(&mut self, b: u64) { self.1.push(b) }
}

#[test]
fn a_reconcile_takes_budgets_out_in_place() {
    let mut bs = Leaving(map(&[1, 4, 9]), Vec::new());
    let mut q: Queue<u64, 4> = Queue::new();
    // One wake per reconcile, so the slots hold 1, 9, 4: not id order.
    reconcile(&mut q, &mut bs, None, &[1]);
    reconcile(&mut q, &mut bs, None, &[1, 9]);
    reconcile(&mut q, &mut bs, None, &[1, 9, 4]);
    assert!(bs.1.is_empty());
    // The running budget stays; the others leave in the order they are visited, 1 then 4, and one
    // pass over the slots takes them out (4, the last, moved into 1's slot and read again).
    reconcile(&mut q, &mut bs, Some(9), &[]);
    assert_eq!(bs.1, [1, 4]);
    assert!(q.contains(9) && !q.contains(1) && !q.contains(4));
    reconcile(&mut q, &mut bs, None, &[]);
    assert_eq!(bs.1, [1, 4, 9]);
    assert!(q.is_empty());
}

#[test]
fn the_floor_survives_an_empty_queue() {
    let mut bs = map(&[1, 2]);
    let mut q: Queue<u64, 4> = Queue::new();
    reconcile(&mut q, &mut bs, None, &[2]);
    q.fold(&mut bs, 2, 50_000);
    let floor = q.floor;
    assert!(floor > 0);
    // 2 blocks: the queue is empty; the counters reset, the floor stays.
    q.deschedule(&mut bs, 2, false);
    assert!(q.is_empty());
    assert_eq!((q.front, q.back, q.floor), (0, 0, floor));
    // 1, asleep all along at pass 0, wakes into the empty queue at the floor.
    reconcile(&mut q, &mut bs, None, &[1]);
    assert_eq!(bs.state(1).pass, floor);
}

#[test]
fn a_running_budget_stays_queued_and_counts_for_the_floor() {
    let mut bs = map(&[1, 2]);
    let mut q: Queue<u64, 4> = Queue::new();
    reconcile(&mut q, &mut bs, None, &[1, 2]);
    // 1 is running and has no other runnable thread listed; it stays in the queue.
    reconcile(&mut q, &mut bs, Some(1), &[2]);
    assert!(q.contains(1));
    reconcile(&mut q, &mut bs, None, &[2]);
    assert!(!q.contains(1));
}

#[test]
fn a_deschedule_charges_at_least_one_unit_and_a_destroy_only_what_ran() {
    let mut bs = map(&[1, 2]);
    bs.0.get_mut(&1).unwrap().1 = 1;
    let mut cpu: Cpu<u64, 4> = Cpu::new();
    cpu.reconcile(&mut bs, &[], &mut [1], |_, b| b == 1);
    // Picked, then off the CPU with nothing seen to run: one unit is charged all the same.
    cpu.switch(&mut bs, Some(1), |_, _| true);
    cpu.switch(&mut bs, None, |_, _| true);
    assert_eq!(bs.state(1).pass, u128::from(MIN_CHARGE * STRIDE));
    // Runs 5, then is destroyed on the CPU: charged exactly 5, and the CPU is free.
    cpu.switch(&mut bs, Some(1), |_, _| true);
    cpu.accrue(5);
    let before = bs.state(1).pass;
    cpu.settle(&mut bs, 1);
    assert_eq!(bs.state(1).pass, before + u128::from(5 * STRIDE));
    cpu.accrue(3);
    cpu.destroy(&mut bs, 1, None, |_| {});
    assert_eq!((cpu.cur, cpu.pending), (None, 0));
}

/// Two harts on one queue: budgets 1 and 2 runnable with one thread each, hart 1 running 1 and
/// hart 0 running 2.
fn two_harts_running() -> (Map, Harts<u64, 4, 2>) {
    let mut bs = map(&[1, 2, 3]);
    let mut harts: Harts<u64, 4, 2> = Harts::new();
    harts.reconcile(&mut bs, &[], &mut [1, 2], |_, b| u32::from(b != 3));
    assert_eq!(harts.pick(&mut bs, |_, b| Some(b)), Some((1, 1)));
    harts.switch(1, &mut bs, Some(1), true, |_, _| true);
    // 1's one thread runs on hart 1: hart 0 is offered nothing of it.
    assert_eq!(harts.pick(&mut bs, |_, b| (b != 1).then_some(b)), Some((2, 2)));
    harts.switch(0, &mut bs, Some(2), true, |_, _| true);
    (bs, harts)
}

#[test]
fn a_budget_running_on_another_hart_stays_queued_through_this_harts_reconcile() {
    let (mut bs, mut harts) = two_harts_running();
    // Hart 0's reconcile lists 1 as lost (its one thread is the one running on hart 1): it stays.
    harts.reconcile(&mut bs, &[1], &mut [], |_, b| u32::from(b == 2));
    assert!(harts.q.contains(1) && bs.state(1).queued);
    // Once no hart runs it, the same reconcile takes it out.
    harts.switch(1, &mut bs, None, false, |_, _| false);
    harts.reconcile(&mut bs, &[1], &mut [], |_, b| u32::from(b == 2));
    assert!(!harts.q.contains(1));
}

#[test]
fn a_pick_passes_over_a_budget_whose_threads_all_run_on_harts() {
    let (mut bs, mut harts) = two_harts_running();
    // Hart 0 leaves 2 and picks again: 1 is lower (it ran nothing), but its one thread runs on
    // hart 1, so 2 is picked and 1 stays queued.
    harts.switch(0, &mut bs, None, true, |_, _| true);
    assert_eq!(harts.pick(&mut bs, |_, b| (b != 1).then_some(b)), Some((2, 2)));
    assert!(harts.q.contains(1) && bs.state(1).queued);
    // With 3 runnable it is still never 1.
    harts.reconcile(&mut bs, &[], &mut [3], |_, _| 1);
    for _ in 0..4 {
        let (b, _) = harts.pick(&mut bs, |_, b| (b != 1).then_some(b)).unwrap();
        assert_ne!(b, 1);
        harts.switch(0, &mut bs, Some(b), true, |_, _| true);
        harts.switch(0, &mut bs, None, true, |_, _| true);
    }
    assert!(harts.q.contains(1));
    // Once hart 1 leaves it, its thread can be picked again.
    harts.switch(1, &mut bs, None, true, |_, _| true);
    assert!(harts.pick(&mut bs, |_, b| Some(b)).is_some());
}

#[test]
fn a_budget_with_two_runnable_threads_runs_on_two_harts_at_one_pass() {
    let mut bs = map(&[1, 2]);
    let mut harts: Harts<u64, 4, 2> = Harts::new();
    harts.reconcile(&mut bs, &[], &mut [1, 2], |_, _| 1);
    // 1 has threads 10 and 11: each hart takes one, 1 ranking lowest both times.
    assert_eq!(harts.pick(&mut bs, |_, b| Some(b * 10)), Some((1, 10)));
    harts.switch(0, &mut bs, Some(1), true, |_, _| true);
    assert_eq!(harts.pick(&mut bs, |_, b| Some(b * 10 + 1)), Some((1, 11)));
    harts.switch(1, &mut bs, Some(1), true, |_, _| true);
    // Queued once, however many harts run it.
    assert_eq!(harts.q.queued().filter(|b| *b == 1).count(), 1);
    // Both runners charge the one pass: a settle folds both.
    let before = bs.state(1).pass;
    harts.accrue(0, 4);
    harts.accrue(1, 6);
    harts.settle(&mut bs, 1);
    assert_eq!(bs.state(1).pass, before + u128::from(STRIDE));
    assert_eq!((harts.runners[0].pending, harts.runners[1].pending), (0, 0));
    // Hart 0 leaves with nothing of 1 runnable: 1 stays queued (hart 1 runs it), requeued behind
    // its equals and charged its minimum.
    let (pass, back) = (bs.state(1).pass, harts.q.back);
    harts.switch(0, &mut bs, None, false, |_, _| false);
    assert!(harts.q.contains(1));
    assert_eq!((bs.state(1).pass, bs.state(1).tie), (pass + u128::from(MIN_CHARGE * STRIDE / 10), back + 1));
    // Hart 1 leaves too: now it goes.
    harts.switch(1, &mut bs, None, false, |_, _| false);
    assert!(!harts.q.contains(1));
}

#[test]
fn a_switch_on_one_hart_leaves_the_others_runner_alone_and_charges_its_minimum() {
    let (mut bs, mut harts) = two_harts_running();
    harts.accrue(1, 7);
    let before = (bs.state(1).pass, bs.state(2).pass);
    // Hart 0 leaves 2 having accrued nothing: MIN_CHARGE for 2 (at the fixture's weight, 10),
    // nothing for 1.
    harts.switch(0, &mut bs, None, true, |_, _| true);
    assert_eq!(bs.state(2).pass, before.1 + u128::from(MIN_CHARGE * STRIDE / 10));
    assert_eq!(bs.state(1).pass, before.0);
    assert_eq!(harts.runners[1], Runner { cur: Some(1), pending: 7 });
    // Work billed to 1 joins hart 1's pending; a settle folds exactly that.
    harts.bill(&mut bs, 1, 3);
    harts.settle(&mut bs, 1);
    assert_eq!(bs.state(1).pass, before.0 + u128::from(STRIDE));
    assert_eq!(harts.runners[1].pending, 0);
    // A destruction clears the runner of the hart that runs it.
    harts.destroy(&mut bs, 1, None, |_| {});
    assert_eq!(harts.runners[1], Runner::new());
}

#[test]
fn a_reconcile_visits_only_the_budgets_it_is_given() {
    let mut bs = Leaving(map(&[1, 2, 3]), Vec::new());
    let mut q: Queue<u64, 4> = Queue::new();
    q.reconcile(&mut bs, |_| false, &[], &mut [2, 1, 2], |_, b| b != 3);
    assert!(q.contains(1) && q.contains(2) && !q.contains(3));
    // 1 has no runnable thread now, but is not among those that lost one: it stays.
    q.reconcile(&mut bs, |_| false, &[2], &mut [3], |_, b| b == 2);
    assert!(bs.1.is_empty() && q.contains(1) && !q.contains(3));
    q.reconcile(&mut bs, |_| false, &[1, 1], &mut [], |_, b| b == 2);
    assert_eq!(bs.1, [1]);
    assert!(!q.contains(1) && q.contains(2));
}

/// A [`Map`] that counts its state reads: what the kernel's checked frame reads cost.
#[derive(Default)]
struct Reads(Map, core::cell::Cell<u64>);

impl Reads {
    fn taken(&self) -> u64 { self.1.take() }
}

impl Budgets<u64> for Reads {
    fn state(&self, b: u64) -> State {
        self.1.set(self.1.get() + 1);
        self.0.state(b)
    }

    fn set_state(&mut self, b: u64, s: State) { self.0.set_state(b, s) }

    fn id(&self, b: u64) -> u64 { b }

    fn weight(&self, b: u64) -> u64 { self.0.weight(b) }
}

/// A queue of `n` budgets, 1 to `n`, all woken in one reconcile, with budget 1 ahead.
fn woken(n: u64) -> (Reads, Queue<u64, 64>) {
    let ids: Vec<u64> = (1..=n).collect();
    let mut bs = Reads(map(&ids), Default::default());
    let mut q: Queue<u64, 64> = Queue::new();
    reconcile(&mut q, &mut bs, None, &ids);
    bs.taken();
    (bs, q)
}

#[test]
fn a_slice_end_reads_no_more_states_for_a_longer_queue() {
    // The floor and the pick read the ranks the queue keeps beside its slots, so a slice end (a
    // fold and a requeue of the running budget, a reconcile that moved nothing, a pick) reads one
    // state for the fold and one for the requeue, whatever the queue holds. The floor moves only
    // when the budget at it is charged or leaves, and then from the ranks: no state read.
    let reads = |n: u64| {
        let (mut bs, mut q) = woken(n);
        let b = q.pick(|_| false).unwrap();
        assert_eq!(bs.taken(), 0, "pick");
        q.fold(&mut bs, b, 100);
        let fold = bs.taken();
        q.deschedule(&mut bs, b, true);
        let requeue = bs.taken();
        q.reconcile(&mut bs, |_| false, &[], &mut [], |_, _| true);
        let reconcile = bs.taken();
        q.pick(|_| false).unwrap();
        assert_eq!(bs.taken(), 0, "pick");
        assert!(q.floor > 0 || n > 1, "the floor rose with the one budget's charge");
        (fold, requeue, reconcile)
    };
    assert_eq!(reads(1), (1, 1, 0));
    assert_eq!(reads(40), reads(1));
    // A budget that leaves at the floor raises it from the ranks, with its own state read only.
    let (mut bs, mut q) = woken(40);
    let b = q.pick(|_| false).unwrap();
    q.deschedule(&mut bs, b, false);
    assert_eq!(bs.taken(), 1);
    assert!(!q.contains(b));
}

#[test]
fn a_rank_out_of_step_with_its_frame_trips_the_audit() {
    let (mut bs, mut q) = woken(3);
    for b in 1..=3 {
        q.fold(&mut bs, b, 10 * b);
        q.deschedule(&mut bs, b, true);
    }
    assert_eq!(q.audit(&bs), Ok(()));
    // A frame written behind the queue's back: its pass, then its tie, then its queued flag.
    let mut s = bs.state(2);
    s.pass += 1;
    bs.set_state(2, s);
    assert_eq!(q.audit(&bs), Err(2));
    s.pass -= 1;
    s.tie += 1;
    bs.set_state(2, s);
    assert_eq!(q.audit(&bs), Err(2));
    s.tie -= 1;
    s.queued = false;
    bs.set_state(2, s);
    assert_eq!(q.audit(&bs), Err(2));
    s.queued = true;
    bs.set_state(2, s);
    assert_eq!(q.audit(&bs), Ok(()));
    // A floor left below the minimum (budget 1, charged least): a raise missed.
    assert_eq!(q.floor, bs.state(1).pass);
    q.floor -= 1;
    assert_eq!(q.audit(&bs), Err(1));
}

/// Budgets with their ready-thread counts, and the processes (slots) in them: `(ready threads,
/// budget)`.
#[derive(Default)]
struct Counted {
    map: Map,
    ready: BTreeMap<u64, u32>,
    slots: Vec<(u32, Option<u64>)>,
}

impl Budgets<u64> for Counted {
    fn state(&self, b: u64) -> State { self.map.state(b) }

    fn set_state(&mut self, b: u64, s: State) { self.map.set_state(b, s) }

    fn id(&self, b: u64) -> u64 { b }

    fn weight(&self, b: u64) -> u64 { self.map.weight(b) }

    fn live(&self, b: u64) -> bool { self.map.0.contains_key(&b) }
}

impl Ready<u64> for Counted {
    fn ready(&self, b: u64) -> u32 { self.ready.get(&b).copied().unwrap_or(0) }

    fn set_ready(&mut self, b: u64, n: u32) { self.ready.insert(b, n); }
}

fn now(bs: &Counted, i: usize) -> (u32, Option<u64>) { bs.slots[i] }

#[test]
fn a_settle_moves_counts_between_budgets() {
    let mut bs = Counted { map: map(&[1, 2]), slots: vec![(0, None); 4], ..Counted::default() };
    let mut marks: Marks<u64, 4> = Marks::new(0);
    bs.slots[0] = (2, Some(1));
    bs.slots[1] = (1, Some(1));
    marks.mark(0);
    marks.mark(1);
    marks.mark(0);
    marks.settle(&mut bs, now);
    assert_eq!(bs.ready(1), 3);
    assert_eq!(marks.changed().1, [1, 1]);
    marks.clear();
    // Slot 0 ends and slot 1's thread blocks: budget 1 has none, and lost them twice.
    bs.slots[0] = (0, None);
    bs.slots[1] = (0, Some(1));
    marks.mark(0);
    marks.mark(1);
    marks.settle(&mut bs, now);
    assert_eq!(bs.ready(1), 0);
    assert_eq!(marks.changed(), (&[1, 1][..], &mut [][..]));
    marks.clear();
    // Slot 0 is a new process in budget 2; budget 1 was destroyed meanwhile, so nothing is
    // taken from it.
    bs.map.0.remove(&1);
    bs.slots[0] = (1, Some(2));
    marks.mark(0);
    marks.settle(&mut bs, now);
    assert_eq!(bs.ready(2), 1);
    assert_eq!(marks.changed(), (&[][..], &mut [2][..]));
}

#[test]
fn a_missed_mark_trips_the_audit() {
    let mut bs = Counted { map: map(&[1, 2, 3]), slots: vec![(0, None); 4], ..Counted::default() };
    let mut marks: Marks<u64, 4> = Marks::new(0);
    let mut q: Queue<u64, 4> = Queue::new();
    // A slice, on a fake clock.
    const EVERY: u64 = 10;
    // One kernel entry at time `t` and the pick after it, as the kernel runs them: the full walk's
    // answer when it is due ([`Marks::audit_due`]), `None` when it is not. Every reconcile's own
    // check passes throughout.
    let entry = |bs: &mut Counted, marks: &mut Marks<u64, 4>, q: &mut Queue<u64, 4>, t: u64| {
        marks.settle(bs, now);
        let (lost, gained) = marks.changed();
        q.reconcile(bs, |_| false, lost, gained, |bs, b| bs.ready(b) > 0);
        assert_eq!(marks.check_visited(bs, |_| false), Ok(()));
        marks.clear();
        let idle = q.pick(|_| false).is_none();
        marks.audit_due(t, EVERY, idle).then(|| marks.audit(bs, q, |_| false, 0..4, now))
    };
    bs.slots[0] = (1, Some(1));
    bs.slots[1] = (1, Some(2));
    marks.mark(0);
    marks.mark(1);
    assert_eq!(entry(&mut bs, &mut marks, &mut q, 100), Some(Ok(())));
    // Slot 1's thread blocks with no mark: budget 2 stays queued with nothing to run. An entry
    // that visits nothing does not walk, nor does one that visits a budget within a slice of the
    // last walk; the first that visits one after a slice finds it.
    bs.slots[1] = (0, Some(2));
    assert_eq!(entry(&mut bs, &mut marks, &mut q, 101), None);
    bs.slots[3] = (1, Some(3));
    marks.mark(3);
    assert_eq!(entry(&mut bs, &mut marks, &mut q, 105), None);
    assert_eq!(entry(&mut bs, &mut marks, &mut q, 115), None);
    bs.slots[3] = (2, Some(3));
    marks.mark(3);
    assert_eq!(
        entry(&mut bs, &mut marks, &mut q, 116),
        Some(Err(Missed::Slot { slot: 1, counted: 1, has: 0 }))
    );
    marks.mark(1);
    assert_eq!(entry(&mut bs, &mut marks, &mut q, 130), Some(Ok(())));
    assert!(!q.contains(2));
    // Every thread blocks, marked, and the hart idles.
    bs.slots[0] = (0, Some(1));
    bs.slots[3] = (0, Some(3));
    marks.mark(0);
    marks.mark(3);
    assert_eq!(entry(&mut bs, &mut marks, &mut q, 131), Some(Ok(())));
    assert!(q.is_empty());
    // A lone missed mark: a thread readied with none, so budget 2 never wakes. The entry visits
    // nothing, within a slice, but nothing is queued: the walk before the idle finds it.
    bs.slots[2] = (1, Some(2));
    assert_eq!(
        entry(&mut bs, &mut marks, &mut q, 132),
        Some(Err(Missed::Slot { slot: 2, counted: 0, has: 1 }))
    );
    // Marked, it wakes; within a slice of that walk, no other runs.
    marks.mark(2);
    assert_eq!(entry(&mut bs, &mut marks, &mut q, 133), None);
    assert_eq!(marks.audit(&bs, &q, |_| false, 0..4, now), Ok(()));
    // A process that ended with no mark: no longer live, still counted.
    assert_eq!(marks.audit(&bs, &q, |_| false, 0..2, now), Err(Missed::Slot { slot: 2, counted: 1, has: 0 }));
    // A count out of step with its slots, and a queue out of step with the counts.
    bs.ready.insert(2, 5);
    assert_eq!(marks.audit(&bs, &q, |_| false, 0..4, now), Err(Missed::Count { id: 2, kept: 5, sum: 1 }));
    bs.ready.insert(2, 1);
    q.deschedule(&mut bs, 2, false);
    assert_eq!(marks.audit(&bs, &q, |_| false, 0..4, now), Err(Missed::Queue { id: 2, queued: false }));
}

#[test]
fn a_wrong_reconcile_trips_its_own_check() {
    let mut bs = Counted { map: map(&[1, 2]), slots: vec![(0, None); 2], ..Counted::default() };
    let mut marks: Marks<u64, 2> = Marks::new(0);
    let mut q: Queue<u64, 2> = Queue::new();
    bs.slots[0] = (1, Some(1));
    bs.slots[1] = (1, Some(2));
    marks.mark(0);
    marks.mark(1);
    marks.settle(&mut bs, now);
    // A reconcile that takes budget 2 for not runnable leaves it asleep with a ready thread: the
    // check of the budgets it visited says so at once.
    let (lost, gained) = marks.changed();
    q.reconcile(&mut bs, |_| false, lost, gained, |_, b| b != 2);
    assert_eq!(marks.check_visited(&bs, |_| false), Err(Missed::Queue { id: 2, queued: false }));
    marks.clear();
    // And one that keeps a budget queued with none.
    marks.mark(1);
    marks.settle(&mut bs, now);
    let (lost, gained) = marks.changed();
    q.reconcile(&mut bs, |_| false, lost, gained, |_, _| true);
    assert_eq!(marks.check_visited(&bs, |_| false), Ok(()));
    marks.clear();
    bs.slots[0] = (0, Some(1));
    marks.mark(0);
    marks.settle(&mut bs, now);
    let (lost, gained) = marks.changed();
    q.reconcile(&mut bs, |_| false, lost, gained, |_, _| true);
    assert_eq!(marks.check_visited(&bs, |_| false), Err(Missed::Queue { id: 1, queued: true }));
    // The running budget may stay queued with none.
    assert_eq!(marks.check_visited(&bs, |b| b == 1), Ok(()));
}

#[test]
fn the_marked_reconcile_matches_a_full_one() {
    // Random ready-thread changes in eight processes over five budgets, some processes ending and
    // others starting in their slots, with runs between: a reconcile of only the marked budgets
    // leaves every state as one that visits them all.
    let ids = [1, 2, 3, 4, 5];
    let mut rng = Rng(23);
    let mut bs = Counted { map: map(&ids), slots: vec![(0, None); 8], ..Counted::default() };
    let mut full = map(&ids);
    let mut marks: Marks<u64, 8> = Marks::new(0);
    let mut q: Queue<u64, 8> = Queue::new();
    let mut fq: Queue<u64, 8> = Queue::new();
    for step in 0..20_000 {
        match rng.range(0, 9) {
            0..=5 => {
                let i = rng.range(0, 7) as usize;
                let b = match bs.slots[i] {
                    (_, Some(b)) if rng.range(0, 9) > 0 => Some(b),
                    _ => (rng.range(0, 5) > 0).then(|| ids[rng.range(0, 4) as usize]),
                };
                bs.slots[i] = (rng.range(0, 2) as u32, b);
                marks.mark(i);
            }
            6 | 7 => {
                marks.settle(&mut bs, now);
                let (lost, gained) = marks.changed();
                q.reconcile(&mut bs, |_| false, lost, gained, |bs, b| bs.ready(b) > 0);
                assert_eq!(marks.check_visited(&bs, |_| false), Ok(()), "step {step}");
                marks.clear();
                let runnable: Vec<u64> = ids
                    .iter()
                    .copied()
                    .filter(|b| bs.slots.iter().any(|s| s.0 > 0 && s.1 == Some(*b)))
                    .collect();
                reconcile(&mut fq, &mut full, None, &runnable);
                assert_eq!(marks.audit(&bs, &q, |_| false, 0..8, now), Ok(()), "step {step}");
                assert_eq!((q.audit(&bs), fq.audit(&full)), (Ok(()), Ok(())), "step {step} ranks");
                for b in ids {
                    assert_eq!(bs.state(b), full.state(b), "step {step} budget {b}");
                }
                assert_eq!((q.floor, q.front, q.back), (fq.floor, fq.front, fq.back), "step {step}");
            }
            _ => {
                // The lowest-ranked runs and is requeued if it still has a ready thread.
                if let Some(b) = q.pick(|_| false) {
                    assert_eq!(fq.pick(|_| false), Some(b), "step {step}");
                    let t = rng.range(1, 1000);
                    let still = bs.ready(b) > 0;
                    q.fold(&mut bs, b, t);
                    assert_eq!(q.audit(&bs), Ok(()), "step {step} fold");
                    q.deschedule(&mut bs, b, still);
                    assert_eq!(q.audit(&bs), Ok(()), "step {step} deschedule");
                    fq.fold(&mut full, b, t);
                    assert_eq!(fq.audit(&full), Ok(()), "step {step} full fold");
                    fq.deschedule(&mut full, b, still);
                    assert_eq!(fq.audit(&full), Ok(()), "step {step} full deschedule");
                }
            }
        }
    }
}

/// Two harts: a heavy budget with one thread runs on one, and its pass lags the light one's on
/// the other. It is capped (900 x 2 > 1 x 1000), so the floor follows the light budget, and a
/// waker enters there, not far behind it.
#[test]
fn a_budget_capped_at_its_threads_is_left_out_of_the_floor() {
    let mut bs = Map::default();
    bs.0.insert(1, (State::default(), 900));
    bs.0.insert(2, (State::default(), 100));
    bs.0.insert(3, (State::default(), 100));
    let mut harts: Harts<u64, 4, 2> = Harts::new();
    harts.set_harts(2, |_| 0, |_| 0);
    harts.reconcile(&mut bs, &[], &mut [1, 2], |_, b| u32::from(b != 3));
    // Each budget's one thread on a hart of its own: 1 on hart 0, 2 on hart 1.
    for (h, b) in [(0, 1), (1, 2)] {
        assert_eq!(harts.pick(&mut bs, |_, x| (x == b).then_some(x)), Some((b, b)));
        harts.switch(h, &mut bs, Some(b), true, |_, _| true);
        harts.set_waiting(b, 0);
    }
    for _ in 0..10 {
        for h in 0..2 {
            harts.accrue(h, 1000);
            harts.settle(&mut bs, harts.cur(h).unwrap());
        }
    }
    assert_eq!(harts.q.capped().collect::<Vec<_>>(), [1]);
    assert!(bs.state(1).pass < bs.state(2).pass);
    assert_eq!(harts.q.floor, bs.state(2).pass, "the floor is the light budget's pass");
    harts.reconcile(&mut bs, &[], &mut [3], |_, b| u32::from(b == 3));
    assert_eq!(bs.state(3).pass, bs.state(2).pass, "the waker enters at the floor");
}

/// A capped budget that gains a thread is no longer capped (900 x 2 < 2 x 1100), and is lifted to
/// the floor: it cannot bank what it had no thread to run.
#[test]
fn a_budget_no_longer_capped_is_lifted_to_the_floor() {
    let mut bs = Map::default();
    bs.0.insert(1, (State::default(), 900));
    bs.0.insert(2, (State::default(), 100));
    bs.0.insert(3, (State::default(), 100));
    let mut harts: Harts<u64, 4, 2> = Harts::new();
    harts.set_harts(2, |_| 0, |_| 0);
    harts.reconcile(&mut bs, &[], &mut [1, 2, 3], |_, _| 1);
    let (b, _) = harts.pick(&mut bs, |_, b| Some(b)).unwrap();
    assert_eq!(b, 1);
    harts.switch(0, &mut bs, Some(1), true, |_, _| true);
    harts.set_waiting(1, 0);
    for _ in 0..10 {
        harts.accrue(0, 1000);
        harts.settle(&mut bs, 1);
        // 2 and 3 take turns on hart 1; 1's one thread runs on hart 0.
        let (b, _) = harts.pick(&mut bs, |_, b| (b != 1).then_some(b)).unwrap();
        harts.switch(1, &mut bs, Some(b), true, |_, _| true);
        harts.accrue(1, 1000);
        harts.switch(1, &mut bs, None, true, |_, _| true);
    }
    assert_eq!(harts.q.capped().collect::<Vec<_>>(), [1]);
    assert!(bs.state(1).pass < harts.q.floor);
    harts.reconcile(&mut bs, &[1], &mut [1], |_, b| if b == 1 { 1 } else { 0 });
    assert_eq!(harts.q.capped().count(), 0);
    assert_eq!(bs.state(1).pass, harts.q.floor, "lifted to the floor");
}

/// One hart keeps no counts; a second hart coming online counts each queued budget's waiting
/// threads and runners afresh, so the next floor raise finds the cap set as if they had been kept.
#[test]
fn a_hart_coming_online_counts_the_queued_budgets_afresh() {
    let mut bs = Map::default();
    bs.0.insert(1, (State::default(), 900));
    bs.0.insert(2, (State::default(), 100));
    let mut harts: Harts<u64, 4, 2> = Harts::new();
    harts.reconcile(&mut bs, &[], &mut [1, 2], |_, _| 1);
    assert_eq!(harts.pick(&mut bs, |_, b| Some(b)), Some((1, 1)));
    harts.switch(0, &mut bs, Some(1), true, |_, _| true);
    // 1's one thread runs on hart 0; 2's waits.
    harts.set_harts(2, |b| u32::from(b == 2), |b| bs.0[&b].1);
    harts.accrue(0, 1000);
    harts.settle(&mut bs, 1);
    assert_eq!(harts.q.capped().collect::<Vec<_>>(), [1], "900 x 2 > 1 x 1000: 1 is capped");
}
