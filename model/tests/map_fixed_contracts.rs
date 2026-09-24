//! `map_fixed` (KERNEL-SPEC.md R11, answer 172): the model's own checks, independent of the
//! kernel boot bench (`tests/programs/src/bin/map-fixed-attack.rs` covers the kernel).

mod common;
use common::contracts::*;
use redoubt_model::{
    kernel::{KERNEL_CHOSEN_BASE, USER_TOP},
    spec::*,
    syscall::*,
};

/// The syscall's own `Result<Ret, Error>`, unwrapping the step machinery around it.
fn call(w: &mut World, tid: u64, s: Syscall) -> Result<Ret, Error> {
    match w.sys(tid, s).unwrap().outcome {
        Outcome::Done(r) => r,
        x => panic!("expected a value or an error, got {x:?}"),
    }
}

/// Page 0 is user space (K5a-addr0): `map_fixed(0, ...)` succeeds, and a second identical call
/// is refused as an overlap, not as "outside user space".
#[test]
fn addr_zero_succeeds_then_a_second_call_overlaps() {
    let mut w = World::new(None);
    let before = w.k.budgets[&1].pages_used;
    assert_eq!(call(&mut w, 1, Syscall::MapFixed { addr: 0, len: PAGE_SIZE, flags: FLAG_R | FLAG_W }), Ok(Ret::Unit));
    let after_first = w.k.budgets[&1].pages_used;
    assert!(after_first > before, "the page (and any page tables it needed) must be charged");
    assert!(w.k.processes[&1].space.contains_key(&0));

    let r = call(&mut w, 1, Syscall::MapFixed { addr: 0, len: PAGE_SIZE, flags: FLAG_R | FLAG_W });
    assert_eq!(r, Err(Error::InvalidArgument));
    assert_eq!(w.k.budgets[&1].pages_used, after_first, "the second call must charge nothing");
}

/// A `map_fixed` placed high (inside `[KERNEL_CHOSEN_BASE, USER_TOP)`, where a static ELF's
/// stub-placed segments and the loader stub itself live) must not make a later `map_anon` fail:
/// `alloc_va`'s first-fit fallback (P1-1) keeps the model as permissive as the kernel's bounded
/// `find_virtual_address` window, which never sees a mapping that high.
#[test]
fn map_fixed_near_user_top_does_not_starve_map_anon() {
    let mut w = World::new(None);
    let high = USER_TOP - PAGE_SIZE;
    assert_eq!(call(&mut w, 1, Syscall::MapFixed { addr: high, len: PAGE_SIZE, flags: FLAG_R }), Ok(Ret::Unit));
    let r = call(&mut w, 1, Syscall::MapAnon { len: PAGE_SIZE, flags: FLAG_R | FLAG_W });
    assert!(matches!(r, Ok(Ret::Addr(_))), "map_anon after a high map_fixed: {r:?}");
}

/// A hostile huge `len` must be refused promptly: the pages-vs-budget check runs before
/// `tables_needed` ever walks the range (P1-2/N2), so this returns at once instead of iterating
/// millions of pages a small budget was never going to afford.
#[test]
fn huge_len_is_refused_promptly() {
    let mut w = World::new(None);
    let before = w.k.budgets[&1].pages_used;
    let r = call(&mut w, 1, Syscall::MapFixed { addr: PAGE_SIZE, len: USER_TOP - PAGE_SIZE, flags: FLAG_R });
    assert_eq!(r, Err(Error::OutOfMemory));
    assert_eq!(w.k.budgets[&1].pages_used, before);
}

/// W+X and W-without-R are refused, nothing mapped and nothing charged, matching `map_anon`'s
/// and `process_map`'s rule (R11).
#[test]
fn bad_flags_are_refused() {
    for flags in [FLAG_W | FLAG_X, FLAG_W, 0, 16] {
        let mut w = World::new(None);
        let before = w.k.budgets[&1].pages_used;
        let r = call(&mut w, 1, Syscall::MapFixed { addr: 0x2000_0000, len: PAGE_SIZE, flags });
        assert_eq!(r, Err(Error::InvalidArgument), "flags {flags:#x}");
        assert_eq!(w.k.budgets[&1].pages_used, before);
        assert!(!w.k.processes[&1].space.contains_key(&(0x2000_0000 / PAGE_SIZE)));
    }
}

/// Unaligned addr/len, `len == 0`, `addr + len` overflow, and a range outside user space are all
/// refused with nothing mapped and nothing charged.
#[test]
fn bad_ranges_are_refused() {
    let cases = [
        (0x2000_0001, PAGE_SIZE),                 // addr unaligned
        (0x2000_0000, PAGE_SIZE + 1),              // len unaligned
        (0x2000_0000, 0),                          // len 0
        (u64::MAX & !(PAGE_SIZE - 1), PAGE_SIZE),  // addr + len overflows
        (USER_TOP, PAGE_SIZE),                     // outside user space
        (USER_TOP - PAGE_SIZE / 2, PAGE_SIZE),     // straddles USER_TOP (also unaligned)
    ];
    for (addr, len) in cases {
        let mut w = World::new(None);
        let before = w.k.budgets[&1].pages_used;
        let r = call(&mut w, 1, Syscall::MapFixed { addr, len, flags: FLAG_R });
        assert_eq!(r, Err(Error::InvalidArgument), "addr {addr:#x} len {len:#x}");
        assert_eq!(w.k.budgets[&1].pages_used, before);
    }
}

/// A range that partially overlaps an existing mapping is refused whole: nothing is mapped, not
/// even the free pages in the range, and nothing is charged.
#[test]
fn partial_overlap_is_refused_whole() {
    let mut w = World::new(None);
    assert_eq!(
        call(&mut w, 1, Syscall::MapFixed { addr: 0x3000_1000, len: PAGE_SIZE, flags: FLAG_R }),
        Ok(Ret::Unit)
    );
    let before = w.k.budgets[&1].pages_used;
    // [free, occupied, free]: addr 0x3000_0000 covers the already-mapped 0x3000_1000 page.
    let r = call(&mut w, 1, Syscall::MapFixed { addr: 0x3000_0000, len: 3 * PAGE_SIZE, flags: FLAG_R });
    assert_eq!(r, Err(Error::InvalidArgument));
    assert_eq!(w.k.budgets[&1].pages_used, before);
    assert!(!w.k.processes[&1].space.contains_key(&(0x3000_0000 / PAGE_SIZE)));
    assert!(!w.k.processes[&1].space.contains_key(&(0x3000_2000 / PAGE_SIZE)));
}

/// The success path: `unmap` returns the budget to its exact baseline.
#[test]
fn success_then_unmap_returns_to_baseline() {
    let mut w = World::new(None);
    let before = w.k.budgets[&1].pages_used;
    let addr = 0x4000_0000;
    assert_eq!(
        call(&mut w, 1, Syscall::MapFixed { addr, len: PAGE_SIZE, flags: FLAG_R | FLAG_W }),
        Ok(Ret::Unit)
    );
    assert!(w.k.budgets[&1].pages_used > before);
    assert_eq!(call(&mut w, 1, Syscall::Unmap { addr, len: PAGE_SIZE }), Ok(Ret::Unit));
    assert_eq!(w.k.budgets[&1].pages_used, before, "unmap must return usage to baseline");
}

/// `KERNEL_CHOSEN_BASE` stays the boundary the kernel picks its own addresses above; a
/// `map_fixed` just below it must not affect that (sanity check for the P1-1 fallback range).
#[test]
fn map_fixed_below_kernel_chosen_base_is_unaffected() {
    let mut w = World::new(None);
    let addr = KERNEL_CHOSEN_BASE - PAGE_SIZE;
    assert_eq!(call(&mut w, 1, Syscall::MapFixed { addr, len: PAGE_SIZE, flags: FLAG_R }), Ok(Ret::Unit));
    let r = call(&mut w, 1, Syscall::MapAnon { len: PAGE_SIZE, flags: FLAG_R | FLAG_W });
    assert!(matches!(r, Ok(Ret::Addr(_))), "map_anon after a low map_fixed: {r:?}");
}
