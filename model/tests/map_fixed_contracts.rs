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
    assert_eq!(
        call(&mut w, 1, Syscall::MapFixed { addr: 0, len: PAGE_SIZE, flags: FLAG_R | FLAG_W }),
        Ok(Ret::Unit)
    );
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
    assert_eq!(
        call(&mut w, 1, Syscall::MapFixed { addr: high, len: PAGE_SIZE, flags: FLAG_R }),
        Ok(Ret::Unit)
    );
    let r = call(&mut w, 1, Syscall::MapAnon { len: PAGE_SIZE, flags: FLAG_R | FLAG_W });
    assert!(matches!(r, Ok(Ret::Addr(_))), "map_anon after a high map_fixed: {r:?}");
}

/// A hostile huge `len` must be refused promptly: the overlap check is one `BTreeMap` range
/// lookup and the pages-vs-budget check runs before `tables_needed` ever walks the range
/// (P1-2/N2). The range matches the boot bench's (4 GiB to the top of user space, about 2^26
/// pages). The bound is generous for a debug build: the fixed path is microseconds, while a
/// regression to one lookup per page, or to `tables_needed` first (2^26 vpns into a
/// `BTreeSet`), takes seconds.
#[test]
fn huge_len_is_refused_promptly() {
    let mut w = World::new(None);
    let before = w.k.budgets[&1].pages_used;
    let addr = 0x1_0000_0000;
    let start = std::time::Instant::now();
    let r = call(&mut w, 1, Syscall::MapFixed { addr, len: USER_TOP - addr, flags: FLAG_R });
    let elapsed = start.elapsed();
    assert_eq!(r, Err(Error::OutOfMemory));
    assert_eq!(w.k.budgets[&1].pages_used, before);
    assert!(elapsed < std::time::Duration::from_secs(1), "took {elapsed:?}");
}

/// Free pages in the budget process 1 runs in.
fn free(w: &World) -> u64 {
    let b = &w.k.budgets[&w.k.processes[&1].budget];
    b.pages_limit - b.pages_used
}

fn used(w: &World) -> u64 { w.k.budgets[&w.k.processes[&1].budget].pages_used }

/// The "page tables" half of the charge check (red team P2-1), which the "pages" half never
/// reaches: pages that fit, whose page tables do not. The same case as the boot bench's
/// `page_table_charge`. `[4 GiB - PAGE, 4 GiB + PAGE)` is the last page of the fourth gigabyte
/// and the first of the fifth, both empty, so each page needs a level-1 table (one per
/// gigabyte) and a level-0 table (one per 2 MiB): 2 pages + 4 tables = 6. The budget is filled
/// to exactly 5 free first, with 2 MiB filler tables in gigabyte 2: once a table exists each
/// page in it costs exactly one.
#[test]
fn page_tables_half_of_the_charge_check() {
    const AT: u64 = 0x1_0000_0000 - PAGE_SIZE;
    const PAGES: u64 = 2;
    const TABLES: u64 = 4;
    const FILL: u64 = 0x8000_0000;
    const SPAN: u64 = 2 << 20;
    const SLOTS: u64 = SPAN / PAGE_SIZE - 1;
    let mut w = World::new(None);
    let target = PAGES + TABLES - 1;
    let (mut opened, mut filled) = (0u64, 0u64);
    loop {
        let short = free(&w).checked_sub(target).expect("the filler overshot");
        if short == 0 {
            break;
        }
        let addr = if short > opened * SLOTS - filled {
            opened += 1;
            FILL + (opened - 1) * SPAN
        } else {
            FILL + (filled / SLOTS) * SPAN + (1 + filled % SLOTS) * PAGE_SIZE
        };
        let len = if addr % SPAN == 0 { 1 } else { short.min(SLOTS - filled % SLOTS) };
        assert_eq!(
            call(&mut w, 1, Syscall::MapFixed { addr, len: len * PAGE_SIZE, flags: FLAG_R | FLAG_W }),
            Ok(Ret::Unit)
        );
        if addr % SPAN != 0 {
            filled += len;
        }
    }

    let before = used(&w);
    let r = call(&mut w, 1, Syscall::MapFixed { addr: AT, len: PAGES * PAGE_SIZE, flags: FLAG_R | FLAG_W });
    assert_eq!(r, Err(Error::OutOfMemory), "pages fit, their page tables do not");
    assert_eq!(used(&w), before, "nothing charged");
    assert!(!w.k.processes[&1].space.contains_key(&(AT / PAGE_SIZE)));

    // One page back: the first filler table keeps its other pages, so only the page returns.
    assert_eq!(call(&mut w, 1, Syscall::Unmap { addr: FILL, len: PAGE_SIZE }), Ok(Ret::Unit));
    assert_eq!(free(&w), PAGES + TABLES, "exactly one page freed");
    let before = used(&w);
    let r = call(&mut w, 1, Syscall::MapFixed { addr: AT, len: PAGES * PAGE_SIZE, flags: FLAG_R | FLAG_W });
    assert_eq!(r, Ok(Ret::Unit), "exactly enough for the pages and their page tables");
    assert_eq!(used(&w), before + PAGES + TABLES, "2 pages and 4 page tables");
}

/// Both sides of a lend are occupied (red team P2-4): the borrower's alias (`LentIn`) and the
/// lender's own page (`LentOut`), here in one process, as in the boot bench. The kernel refuses
/// them through their non-empty PTEs; the model through `p.space`, a different mechanism, which
/// is why both are tested.
#[test]
fn lent_in_and_lent_out_pages_are_refused() {
    let mut w = World::new(None);
    let (ep, client, server) = w.setup().unwrap();
    let lend = w.lend().unwrap();
    w.call(client, Some(lend), FOREVER).unwrap();
    let m = w.take(ep, server).unwrap();
    let lent_in = m.buffer.expect("the lend").addr;
    for (addr, what) in [(lent_in, "lent in"), (lend.addr, "lent out")] {
        let before = used(&w);
        let r = call(&mut w, server, Syscall::MapFixed { addr, len: PAGE_SIZE, flags: FLAG_R | FLAG_W });
        assert_eq!(r, Err(Error::InvalidArgument), "{what}");
        assert_eq!(used(&w), before, "{what}: nothing charged");
    }
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
        (0x2000_0000, PAGE_SIZE + 1),             // len unaligned
        (0x2000_0000, 0),                         // len 0
        (u64::MAX & !(PAGE_SIZE - 1), PAGE_SIZE), // addr + len overflows
        (USER_TOP, PAGE_SIZE),                    // outside user space
        (USER_TOP - PAGE_SIZE, 2 * PAGE_SIZE),    // aligned, straddles USER_TOP
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
