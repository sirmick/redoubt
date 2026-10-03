//! The lists on their own, over words in a map: the kernel keeps the same words in its frames.

extern crate std;

use std::collections::BTreeMap;
use std::vec::Vec;

use super::*;

/// Every page's words, zero until written, as a new frame is.
#[derive(Default)]
struct Mem(BTreeMap<(Page, usize), u64>);

impl Words for Mem {
    fn read(&self, page: Page, word: usize) -> u64 { self.0.get(&(page, word)).copied().unwrap_or(0) }

    fn write(&mut self, page: Page, word: usize, value: u64) {
        if value == 0 {
            self.0.remove(&(page, word));
        } else {
            self.0.insert((page, word), value);
        }
    }
}

/// A small deterministic generator (xorshift), so every case is reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

const E: u32 = 7;

fn members(w: &Mem, list: List) -> Vec<u64> {
    let mut out = Vec::new();
    list.audit(w, |_, r| {
        out.push(r);
        Ok(())
    })
    .expect("the list's links hold");
    out
}

/// R2 as the model states it (`model/src/kernel.rs`, `next_sender` and `served`): every queued
/// message carries its due, a take sets it on the group's queued messages, and the pick is the
/// first takeable message of the group whose is due earliest.
#[derive(Default)]
struct Reference {
    /// (thread, group, send, due), in arrival order.
    queue: Vec<(u64, u64, bool, u64)>,
}

impl Reference {
    fn pick(&self, calls: bool) -> u64 {
        let mut best: Option<(u64, u64)> = None;
        let mut seen = Vec::new();
        for &(t, g, send, due) in &self.queue {
            if seen.contains(&g) || !(calls || send) {
                continue;
            }
            seen.push(g);
            if best.is_none_or(|(d, _)| due < d) {
                best = Some((due, t));
            }
        }
        best.map_or(0, |(_, t)| t)
    }

    fn served(&mut self, t: u64, now: u64) {
        let g = self.queue.iter().find(|m| m.0 == t).expect("queued").1;
        for m in self.queue.iter_mut().filter(|m| m.1 == g) {
            m.3 = now;
        }
    }

    fn remove(&mut self, t: u64) { self.queue.retain(|m| m.0 != t) }
}

/// The world a test drives: the lists, the reference, which group each thread sends in, and the
/// one counter.
#[derive(Default)]
struct World {
    w: Mem,
    reference: Reference,
    group: BTreeMap<u64, u64>,
    send: BTreeMap<u64, bool>,
    counter: u64,
}

impl World {
    fn tick(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }

    fn queue(&mut self, t: u64, g: u64, send: bool) {
        let group = &self.group;
        let node = find(&self.w, E, |_, node| group[&node] == g);
        let seq = self.tick();
        enqueue(&mut self.w, E, node, t, send, seq);
        self.group.insert(t, g);
        self.send.insert(t, send);
        self.reference.queue.push((t, g, send, seq));
        self.check();
    }

    /// `t` leaves without a take (a timeout, `Dead`, a kill).
    fn leave(&mut self, t: u64) {
        dequeue(&mut self.w, E, t, self.send[&t]);
        self.reference.remove(t);
        self.group.remove(&t);
        self.check();
    }

    /// A receiver takes (or refuses) what R2 picks for it; returns it.
    fn take(&mut self, calls: bool) -> u64 {
        let t = pick(&self.w, E, calls);
        if t != 0 {
            let now = self.tick();
            served(&mut self.w, E, t, now);
            self.reference.served(t, now);
            self.leave(t);
        }
        t
    }

    fn check(&self) {
        for calls in [true, false] {
            assert_eq!(pick(&self.w, E, calls), self.reference.pick(calls), "pick(calls: {calls})");
        }
        let n = audit_groups(&self.w, E, |_, m, send| {
            if self.send[&m] == send && self.group.contains_key(&m) {
                Ok(())
            } else {
                Err(Fault::Member(Page::Thread(m)))
            }
        })
        .expect("the groups hold");
        assert_eq!(n, self.reference.queue.len());
        assert_eq!(waiting(&self.w, E) as usize, n, "the endpoint counts its queued messages");
        for (&t, &g) in &self.group {
            assert_eq!(self.group[&group_of(&self.w, t)], g, "{t}'s node is in its own group");
        }
    }
}

#[test]
fn receivers_wait_in_arrival_order() {
    let mut w = Mem::default();
    let list = List::receivers(E);
    for t in [0x305, 0x201, 0x402, 0x203] {
        list.push_back(&mut w, t);
    }
    assert_eq!(members(&w, list), [0x305, 0x201, 0x402, 0x203]);
    list.remove(&mut w, 0x201);
    list.remove(&mut w, 0x203);
    list.push_back(&mut w, 0x201);
    assert_eq!(members(&w, list), [0x305, 0x402, 0x201]);
    assert!(list.contains(&w, 0x305) && list.contains(&w, 0x201) && !list.contains(&w, 0x203));
    assert_eq!(list.pop_front(&mut w), 0x305);
    assert_eq!(members(&w, list), [0x402, 0x201]);
}

#[test]
fn a_full_receiver_skips_calls() {
    // A's call at 1, B's send at 2, A's send at 3: a receiver below its limit takes A's call; a
    // full one takes B's send, due before A's oldest send.
    let mut world = World::default();
    world.queue(0x201, 1, false);
    world.queue(0x301, 2, true);
    world.queue(0x202, 1, true);
    assert_eq!(pick(&world.w, E, true), 0x201);
    assert_eq!(pick(&world.w, E, false), 0x301);
    assert_eq!(world.take(false), 0x301);
    // B is gone, and A's send is next for a full receiver; A's call is still first for the rest.
    assert_eq!(pick(&world.w, E, false), 0x202);
    assert_eq!(pick(&world.w, E, true), 0x201);
}

#[test]
fn a_group_moves_on_a_take() {
    let mut world = World::default();
    world.queue(0x201, 1, true);
    world.queue(0x301, 2, true);
    world.queue(0x202, 1, true);
    world.queue(0x401, 3, false);
    // A's turn is due first; served, it goes behind B and C, its second message with it.
    assert_eq!(world.take(true), 0x201);
    assert_eq!(world.take(true), 0x301);
    assert_eq!(world.take(true), 0x401);
    assert_eq!(world.take(true), 0x202);
    assert_eq!(world.take(true), 0);
    assert!(no_groups(&world.w, E));
}

#[test]
fn a_node_passes_to_its_next_sender() {
    let mut world = World::default();
    world.queue(0x201, 1, true);
    world.queue(0x301, 2, false);
    world.queue(0x202, 1, false);
    world.queue(0x203, 1, true);
    assert_eq!(group_of(&world.w, 0x203), 0x201);
    // A's oldest times out: its node goes to 0x202, which every member now names, and A, due at
    // 3, goes behind B, due at 2.
    world.leave(0x201);
    for t in [0x202, 0x203] {
        assert_eq!(group_of(&world.w, t), 0x202);
    }
    assert_eq!(count(&world.w, 0x202), 2);
    assert_eq!(pick(&world.w, E, true), 0x301);
    // A full receiver still finds A's send, its oldest now 0x203.
    assert_eq!(pick(&world.w, E, false), 0x203);
    // The old node keeps none of the group's words.
    assert!(world.w.0.keys().all(|(p, i)| *p != Page::Thread(0x201) || *i == T_SEQ));
}

#[test]
fn a_destruction_empties_every_list() {
    let mut world = World::default();
    for (i, t) in (0x201..0x210).enumerate() {
        world.queue(t, (i % 3) as u64, i % 2 == 0);
    }
    let w = &mut world.w;
    let receivers = List::receivers(E);
    let waiters = List::irq_waiters(9);
    for t in 0x301..0x305 {
        receivers.push_back(w, t);
        waiters.push_back(w, t + 0x100);
    }
    for c in 1..4 {
        List::notices(E).push_front(w, c);
        List::open(E).push_front(w, c + 10);
        List::taken(5).push_front(w, c + 10);
    }
    for t in 0x201..0x210 {
        List::queued(5).push_front(w, t);
    }
    for p in 1..3 {
        List::exits(E).push_back(w, p);
        List::reporters(E).push_front(w, p + 10);
    }
    // The endpoint counts every member of its lists: 15 queued, 4 receivers, 3 notices, 3 open,
    // 2 exits and 2 reporters.
    assert_eq!(waiting(w, E), 29);
    // What `budgets_dying` does: pop each list until it is empty.
    while receivers.pop_front(w) != 0 {}
    while waiters.pop_front(w) != 0 {}
    for list in
        [List::notices(E), List::open(E), List::taken(5), List::queued(5), List::exits(E), List::reporters(E)]
    {
        while list.pop_front(w) != 0 {}
    }
    loop {
        let t = pick(&world.w, E, true);
        if t == 0 {
            break;
        }
        world.leave(t);
    }
    // Nothing is left of any list but each message's own arrival.
    assert!(world.w.0.keys().all(|(_, i)| *i == T_SEQ), "{:?}", world.w.0);
}

#[test]
fn exit_notices_wait_in_the_order_they_came() {
    let mut w = Mem::default();
    let (exits, reporters) = (List::exits(E), List::reporters(E));
    for p in 1..=4 {
        reporters.push_front(&mut w, p);
    }
    // Processes 3, 1 and 4 end in that order: each leaves the reporters for the exits' tail.
    for p in [3, 1, 4] {
        reporters.remove(&mut w, p);
        exits.push_back(&mut w, p);
    }
    assert_eq!(members(&w, exits), [3, 1, 4]);
    assert_eq!(members(&w, reporters), [2]);
    assert_eq!(waiting(&w, E), 4);
    // A notice received leaves from the head.
    assert_eq!(exits.pop_front(&mut w), 3);
    assert_eq!(members(&w, exits), [1, 4]);
    assert_eq!(waiting(&w, E), 3);
}

/// A process's timed waits: a wait leaves by whichever path ends it (answered, woken, expired,
/// its thread destroyed), from anywhere in the list, and a process's end empties it. A wait ended
/// but left on the list (a missed unlink) is what the kernel's audit finds: its member check says
/// the thread waits on nothing.
#[test]
fn a_timed_wait_leaves_by_every_path() {
    let mut w = Mem::default();
    let timed = List::timed(7);
    for t in 0x701..=0x706 {
        timed.push_front(&mut w, t);
    }
    // Answered (the head), woken (the middle), expired (the tail), destroyed (another middle).
    let paths: [(u64, &[u64]); 4] = [
        (0x706, &[0x705, 0x704, 0x703, 0x702, 0x701]),
        (0x703, &[0x705, 0x704, 0x702, 0x701]),
        (0x701, &[0x705, 0x704, 0x702]),
        (0x704, &[0x705, 0x702]),
    ];
    for (t, left) in paths {
        assert!(timed.contains(&w, t));
        timed.remove(&mut w, t);
        assert!(!timed.contains(&w, t));
        assert_eq!(members(&w, timed), left);
    }
    // Another process's slot is its own list.
    List::timed(8).push_front(&mut w, 0x801);
    assert_eq!(members(&w, timed), [0x705, 0x702]);
    // A missed unlink: 0x702's wait ended, but it is still listed.
    let waiting = |t: u64| t != 0x702;
    let stale = timed.audit(&w, |_, t| if waiting(t) { Ok(()) } else { Err(Fault::Member(Page::Thread(t))) });
    assert_eq!(stale, Err(Fault::Member(Page::Thread(0x702))));
    // The process ends: every thread's wait goes, and the list with them.
    while timed.pop_front(&mut w) != 0 {}
    List::timed(8).pop_front(&mut w);
    assert!(w.0.values().all(|v| *v == 0), "{:?}", w.0);
}

#[test]
fn the_groups_follow_the_model() {
    for seed in 1..=200u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut world = World::default();
        let mut next = 0x200;
        for _ in 0..300 {
            match rng.below(4) {
                0 | 1 => {
                    let g = rng.below(5) as u64;
                    if world.reference.queue.iter().filter(|m| m.1 == g).count() < 6 {
                        next += 1;
                        world.queue(next, g, rng.below(2) == 0);
                    }
                }
                2 if !world.reference.queue.is_empty() => {
                    let t = world.reference.queue[rng.below(world.reference.queue.len())].0;
                    world.leave(t);
                }
                _ => {
                    world.take(rng.below(3) != 0);
                }
            }
        }
    }
}

#[test]
fn the_due_list_sorts_stably() {
    let mut w = Mem::default();
    let due = List::due();
    let deadline = |t: u64| [5, 3, 5, 1, 3, 9, 1, 5][(t - 0x201) as usize];
    for t in 0x201..=0x208 {
        due.push_back(&mut w, t);
    }
    due.sort(&mut w, |_, t| deadline(t));
    assert_eq!(members(&w, due), [0x204, 0x207, 0x202, 0x205, 0x201, 0x203, 0x208, 0x206]);
    for n in [0, 1, 2, 7, 64, 100] {
        let mut w = Mem::default();
        let mut rng = Rng(n as u64 + 1);
        let keys: Vec<u64> = (0..n).map(|_| rng.next() % 8).collect();
        for t in 0..n as u64 {
            due.push_back(&mut w, t + 1);
        }
        due.sort(&mut w, |_, t| keys[t as usize - 1]);
        let mut want: Vec<u64> = (1..=n as u64).collect();
        want.sort_by_key(|t| keys[*t as usize - 1]);
        assert_eq!(members(&w, due), want);
    }
}

/// Each list's audit trips when one of its links is corrupted.
#[test]
fn each_audit_trips_on_a_corrupted_link() {
    let lists = [
        List::receivers(E),
        List::irq_waiters(9),
        List::notices(E),
        List::open(E),
        List::queued(5),
        List::taken(5),
        List::due(),
        List::exits(E),
        List::reporters(E),
        List::timed(3),
    ];
    for list in lists {
        for (word, value) in [(list.prev, 0x999), (list.next, 0), (list.next, 3)] {
            let mut w = Mem::default();
            for r in 1..=3 {
                list.push_front(&mut w, r);
            }
            assert_eq!(list.audit(&w, |_, _| Ok(())), Ok(3));
            w.write(list.page(2), word, value);
            // A list with no tail cut short reads as a shorter list: the kernel's audit finds the
            // member it lost in its count of the threads, against which it matches every list.
            assert_ne!(list.audit(&w, |_, _| Ok(())), Ok(3), "{list:?} word {word} = {value}");
        }
        if let Some(tail) = list.tail {
            let mut w = Mem::default();
            for r in 1..=3 {
                list.push_back(&mut w, r);
            }
            w.write(list.owner, tail, 2);
            assert!(list.audit(&w, |_, _| Ok(())).is_err(), "{list:?}'s tail");
        }
    }
    // The groups: a chain's link, a member's node, a node's count, a group out of order.
    let corruptions: [(u64, usize, u64); 5] = [
        (0x202, T_PREV, 0x999),
        (0x203, T_GROUP, 0x203),
        (0x201, G_COUNT, 9),
        (0x201, G_TAKEN, 100),
        (0x201, G_NEXT, 0),
    ];
    for (page, word, value) in corruptions {
        let mut world = World::default();
        world.queue(0x201, 1, true);
        world.queue(0x301, 2, true);
        world.queue(0x202, 1, false);
        world.queue(0x203, 1, false);
        world.w.write(Page::Thread(page), word, value);
        assert!(audit_groups(&world.w, E, |_, _, _| Ok(())).is_err(), "{page:#x} word {word} = {value}");
    }
}
