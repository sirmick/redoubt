//! A range call names at most `MAX_RANGE_PAGES` pages (kernel/memory.md, R22), and the runtime
//! splits a longer one: `map_anon` of a long run asks the kernel's `map_anon` to place the whole
//! run, of which the kernel maps the first range, and maps the rest with `map_fixed`; a refused
//! extension gives back what it mapped and searches again. Unmapping gives a run back a range at a
//! time. The fake kernel places as the kernel does, in each process's own area, and refuses a
//! longer range elsewhere; raw `map_fixed` calls, which leave the search's start where it was, make
//! the gaps, and its scripted refusals stand in for a page another mapping takes meanwhile.

use redoubt_fake_kernel::{AREA_PAGES, Fake, fake};
use redoubt_rt::Transport;
use redoubt_rt::abi::{Call, Error, MAX_RANGE_PAGES, MemFlags, PAGE_SIZE};
use redoubt_rt::handle::map_anon;
use redoubt_rt::ipc::Buffer;

const RANGE: usize = MAX_RANGE_PAGES * PAGE_SIZE;

/// Pages of a run longer than two ranges: two whole ranges and part of a third.
const RUN: usize = 2 * MAX_RANGE_PAGES + 8;

fn rw() -> MemFlags { MemFlags::READ | MemFlags::WRITE }

fn pages(n: usize) -> usize { n * PAGE_SIZE }

/// `(address, length)` of the ranges a run of `n` pages at `at` is mapped as.
fn ranges(at: usize, n: usize) -> Vec<(usize, usize)> {
    (0..pages(n)).step_by(RANGE).map(|done| (at + done, (pages(n) - done).min(RANGE))).collect()
}

/// A raw `map_fixed` of `n` pages at `at`, as the running process, a range at most.
fn pin(f: &Fake, at: usize, n: usize) {
    f.call(&Call::MapFixed { addr: at, len: pages(n), flags: rw() }).unwrap();
}

fn unpin(f: &Fake, at: usize, n: usize) { f.call(&Call::Unmap { addr: at, len: pages(n) }).unwrap(); }

/// The process's area, its first page mapped (the search's start there), and every page from
/// `from` to `to` pinned a range at a time; the area's start.
fn area(f: &Fake, from: usize, to: usize) -> usize {
    let base = map_anon(PAGE_SIZE, rw()).unwrap();
    for at in (from..to).step_by(MAX_RANGE_PAGES) {
        pin(f, base + pages(at), MAX_RANGE_PAGES.min(to - at));
    }
    base
}

/// The calls `pid` made from its `skip`th call on.
fn calls_since(f: &Fake, pid: usize, skip: usize) -> Vec<&'static str> { f.calls(pid)[skip..].to_vec() }

#[test]
fn a_long_run_is_one_run_mapped_a_range_at_a_time() {
    let f = fake();
    let pid = f.process(1001, &[]);
    f.as_process(pid, || {
        let mut run = Buffer::new(RUN).unwrap();
        let at = run.as_ptr() as usize;
        assert_eq!(f.mappings(pid), ranges(at, RUN));
        assert_eq!(f.calls(pid), ["map_anon", "map_fixed", "map_fixed"]);
        assert!(run.iter().all(|b| *b == 0), "every page zeroed");
        run[pages(RUN) - 1] = 7;
        drop(run);
        assert_eq!(f.mappings(pid), [], "unmapped whole");
        assert_eq!(&f.calls(pid)[3..], ["unmap", "unmap", "unmap"]);
    });
}

#[test]
fn a_run_is_placed_by_one_search_past_every_gap_too_short_for_it() {
    let f = fake();
    let pid = f.process(1002, &[]);
    f.as_process(pid, || {
        // A gap of 39 pages after the first, two ranges and more but short of the run, then the
        // rest of the area free: the one search passes the gap, and nothing is mapped in it.
        let base = area(f, 40, 41);
        let (held, calls) = (f.held(pid).1, f.calls(pid).len());
        f.held_peak(pid);
        let at = map_anon(pages(RUN), rw()).unwrap();
        assert_eq!(at, base + pages(41), "past the short gap");
        assert_eq!(calls_since(f, pid, calls), ["map_anon", "map_fixed", "map_fixed"]);
        assert_eq!(
            f.mappings(pid).into_iter().filter(|(a, _)| *a >= at).collect::<Vec<_>>(),
            ranges(at, RUN)
        );
        assert_eq!(f.held_peak(pid), held + RUN, "the run alone, never a page of the gap");
    });
}

#[test]
fn a_run_before_the_search_s_start_is_found_after_the_wrap() {
    let f = fake();
    let pid = f.process(1003, &[]);
    f.as_process(pid, || {
        // All but the area's last 20 pages pinned, a page placed there (the search's start now),
        // then the first three ranges after the area's first page given back: the only gap that
        // holds the run lies before the search's start, and the one after it is too short.
        let base = area(f, 1, AREA_PAGES - 20);
        let late = map_anon(PAGE_SIZE, rw()).unwrap();
        assert_eq!(late, base + pages(AREA_PAGES - 20));
        for at in [1, 17, 33] {
            unpin(f, base + pages(at), MAX_RANGE_PAGES);
        }
        let calls = f.calls(pid).len();
        assert_eq!(map_anon(pages(RUN), rw()), Ok(base + pages(1)));
        assert_eq!(calls_since(f, pid, calls), ["map_anon", "map_fixed", "map_fixed"]);
    });
}

#[test]
fn a_run_no_gap_holds_is_out_of_memory_with_nothing_mapped() {
    let f = fake();
    let pid = f.process(1004, &[]);
    f.as_process(pid, || {
        // As above, but the gap before the search's start holds one range only: the kernel's one
        // search finds no gap for the run.
        let base = area(f, 1, AREA_PAGES - 20);
        map_anon(PAGE_SIZE, rw()).unwrap();
        unpin(f, base + pages(1), MAX_RANGE_PAGES);
        let before = f.mappings(pid);
        let calls = f.calls(pid).len();
        assert_eq!(map_anon(pages(RUN), rw()), Err(Error::OutOfMemory));
        assert_eq!(f.mappings(pid), before, "nothing left mapped");
        assert_eq!(calls_since(f, pid, calls), ["map_anon"]);
    });
}

#[test]
fn a_refused_extension_gives_back_its_ranges_and_searches_again() {
    let f = fake();
    let pid = f.process(1005, &[]);
    // The run's third range finds a page taken, as a mapping made in the moment would: the two
    // ranges mapped are given back, and the search starts again from where it placed the run.
    f.refuse_after(pid, "map_fixed", 1, Error::InvalidArgument);
    f.as_process(pid, || {
        let at = map_anon(pages(RUN), rw()).unwrap();
        assert_eq!(f.mappings(pid), ranges(at, RUN), "one run, nothing of the refused one left");
        assert_eq!(
            f.calls(pid),
            ["map_anon", "map_fixed", "map_fixed", "unmap", "unmap", "map_anon", "map_fixed", "map_fixed"]
        );
    });
}

#[test]
fn a_run_the_budget_cannot_hold_leaves_nothing_mapped() {
    let f = fake();
    let pid = f.process(1006, &[]);
    f.refuse_after(pid, "map_fixed", 1, Error::OutOfMemory);
    f.as_process(pid, || {
        assert_eq!(map_anon(pages(RUN), rw()), Err(Error::OutOfMemory));
        assert_eq!(f.mappings(pid), []);
        assert_eq!(f.calls(pid).iter().filter(|c| **c == "map_anon").count(), 1, "no search again");
    });
}

#[test]
fn a_run_of_one_range_is_one_call() {
    let f = fake();
    let pid = f.process(1007, &[]);
    f.as_process(pid, || {
        let one = map_anon(RANGE, rw()).unwrap();
        assert_eq!(f.mappings(pid), [(one, RANGE)]);
        assert_eq!(f.calls(pid), ["map_anon"]);
    });
}
