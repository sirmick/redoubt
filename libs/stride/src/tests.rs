//! Unit tests of the rules on their own. `tests/differential.rs` checks them against the model.

extern crate std;

use std::collections::BTreeMap;
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

#[test]
fn ranks_follow_all_four_clauses() {
    let mut bs = map(&[1, 2, 3, 4, 5]);
    let mut q: Queue<u64, 8> = Queue::new();
    // Clause 3: wakes in one reconcile run lowest id first.
    q.reconcile(&mut bs, None, &[3, 1, 2]);
    assert_eq!(q.pick(&bs), Some(1));
    // Budget 1 runs a slice and is requeued; its pass is now higher, so 2 is next.
    q.fold(&mut bs, 1, 100);
    q.deschedule(&mut bs, 1, true);
    assert_eq!(q.pick(&bs), Some(2));
    // Clause 2: a later reconcile's wake at the same pass (the floor) goes ahead of earlier ones.
    q.reconcile(&mut bs, None, &[1, 2, 3, 4]);
    assert_eq!(q.pick(&bs), Some(4));
    // Clause 1 and 4: requeued budgets at an equal pass go behind wakers, in FIFO order.
    q.fold(&mut bs, 4, 0);
    q.deschedule(&mut bs, 4, true);
    q.fold(&mut bs, 2, 0);
    q.deschedule(&mut bs, 2, true);
    q.fold(&mut bs, 3, 0);
    q.deschedule(&mut bs, 3, true);
    // 4, 2, 3 are requeued (in that order) at the floor; a new waker 5 at the floor goes first.
    q.reconcile(&mut bs, None, &[1, 2, 3, 4, 5]);
    let order: Vec<u64> = (0..4)
        .map(|_| {
            let b = q.pick(&bs).unwrap();
            q.fold(&mut bs, b, 1000);
            q.deschedule(&mut bs, b, true);
            b
        })
        .collect();
    assert_eq!(order, [5, 4, 2, 3]);
}

#[test]
fn the_floor_survives_an_empty_queue() {
    let mut bs = map(&[1, 2]);
    let mut q: Queue<u64, 4> = Queue::new();
    q.reconcile(&mut bs, None, &[2]);
    q.fold(&mut bs, 2, 50_000);
    let floor = q.floor;
    assert!(floor > 0);
    // 2 blocks: the queue is empty; the counters reset, the floor stays.
    q.deschedule(&mut bs, 2, false);
    assert!(q.is_empty());
    assert_eq!((q.front, q.back, q.floor), (0, 0, floor));
    // 1, asleep all along at pass 0, wakes into the empty queue at the floor.
    q.reconcile(&mut bs, None, &[1]);
    assert_eq!(bs.state(1).pass, floor);
}

#[test]
fn a_running_budget_stays_queued_and_counts_for_the_floor() {
    let mut bs = map(&[1, 2]);
    let mut q: Queue<u64, 4> = Queue::new();
    q.reconcile(&mut bs, None, &[1, 2]);
    // 1 is running and has no other runnable thread listed; it stays in the queue.
    q.reconcile(&mut bs, Some(1), &[2]);
    assert!(q.contains(1));
    q.reconcile(&mut bs, None, &[2]);
    assert!(!q.contains(1));
}

#[test]
fn a_deschedule_charges_at_least_one_unit_and_a_destroy_only_what_ran() {
    let mut bs = map(&[1, 2]);
    bs.0.get_mut(&1).unwrap().1 = 1;
    let mut cpu: Cpu<u64, 4> = Cpu::new();
    cpu.reconcile(&mut bs, &[1]);
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
