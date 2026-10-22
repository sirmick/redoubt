//! An allocation one process asks the VM for is checked before it is made (docs/userland/
//! beamlet.md, "Limits inside one VM"): past the process's heap limit, or refused by the
//! allocator, the process is killed as `max_heap_size` kills it, and the VM serves on. This
//! binary's allocator refuses any one allocation over [`MOST`] bytes (or another cap a test sets
//! for its thread), and on a thread that sets one, any past what that thread may hold, as a VM's
//! free pages refuse one past them; before
//! the check, such a refusal ended the whole test binary, as it ended a session's VM. A process
//! sets that for its thread by displaying `{pages, KiB}`: from then on the thread may hold only
//! that much more. The fixture's source is `src/allocation.erl`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::BTreeMap;

use beamlet_vm::Vm;
use beamlet_vm::platform::{Lookup, Platform, PlatformError};
use beamlet_vm::vm::Limits;

/// The largest single allocation this binary's allocator gives.
const MOST: usize = 1 << 20;

thread_local! {
    /// The largest single allocation this thread is given ([`MOST`], unless a test sets it).
    static ONE: Cell<usize> = const { Cell::new(MOST) };
    /// The bytes this thread may hold at once (all, unless a test sets it), and holds now.
    static PAGES: Cell<usize> = const { Cell::new(usize::MAX) };
    static HELD: Cell<usize> = const { Cell::new(0) };
}

/// Whether this thread may go from holding `old` bytes of a block to `new`, counting it if so.
fn take(old: usize, new: usize) -> bool {
    if new > ONE.try_with(Cell::get).unwrap_or(MOST) {
        return false;
    }
    let pages = PAGES.try_with(Cell::get).unwrap_or(usize::MAX);
    HELD.try_with(|held| {
        let after = held.get().saturating_sub(old).saturating_add(new);
        if after > pages {
            return false;
        }
        held.set(after);
        true
    })
    .unwrap_or(true)
}

/// The system's allocator, refusing (null) any one allocation over the thread's cap, or past what
/// the thread may hold.
struct Capped;

// SAFETY: every call the caps let through is passed straight to the system's allocator; a
// refusal is a null pointer, which the `GlobalAlloc` contract allows for any request.
unsafe impl GlobalAlloc for Capped {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !take(0, layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        take(layout.size(), 0);
        // SAFETY: as for `alloc`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if !take(layout.size(), new_size) {
            return std::ptr::null_mut();
        }
        // SAFETY: as for `alloc`; a refused growth leaves the old block the caller's.
        let moved = unsafe { System.realloc(ptr, layout, new_size) };
        if moved.is_null() {
            take(new_size, layout.size());
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Capped = Capped;

struct TestPlatform {
    now: u64,
}

impl Platform for TestPlatform {
    fn monotonic_us(&mut self) -> u64 {
        self.now += 1;
        self.now
    }

    fn system_time_us(&mut self) -> Option<u64> { None }

    fn idle(&mut self, deadline: Option<u64>) {
        if let Some(d) = deadline {
            self.now = self.now.max(d);
        }
    }

    /// `erlang:display({pages, KiB})`: from here this thread may hold only `KiB` more.
    fn console_write(&mut self, bytes: &[u8]) {
        let kib = bytes
            .strip_prefix(b"{pages,")
            .and_then(|b| b.strip_suffix(b"}\n"))
            .and_then(|b| std::str::from_utf8(b).ok()?.parse::<usize>().ok());
        if let Some(kib) = kib {
            PAGES.with(|p| p.set(HELD.with(Cell::get) + kib * 1024));
        }
    }

    fn random(&mut self, _buf: &mut [u8]) -> Result<(), PlatformError> { Err(PlatformError::Unavailable) }

    fn load_module(&mut self, module: &str) -> Lookup {
        let modules: BTreeMap<&str, &[u8]> =
            [("allocation", include_bytes!("fixtures/allocation.beam").as_slice())].into_iter().collect();
        modules.get(module).map_or(Lookup::Absent, |b| Lookup::Found(b.to_vec()))
    }
}

/// Run `allocation:f()` and return its result as text: how the asking process ended, and the
/// length of a list the VM built after it.
fn run(f: &str) -> String { run_capped(f, MOST) }

/// As [`run`], the largest single allocation given being `one` bytes.
fn run_capped(f: &str, one: usize) -> String {
    ONE.with(|o| o.set(one));
    let mut vm = Vm::with_limits(Box::new(TestPlatform { now: 0 }), Limits::default());
    let pid = vm.spawn("allocation", f, |_| Vec::new()).unwrap();
    let r = vm.run_bounded(pid, 10_000_000).expect("finished");
    PAGES.with(|p| p.set(usize::MAX));
    ONE.with(|o| o.set(MOST));
    match r.unwrap() {
        Ok(t) => t.to_string(),
        Err(e) => format!("raised {}", e.reason),
    }
}

#[test]
fn a_heap_the_allocator_will_not_grow_kills_its_process() {
    assert_eq!(run("heap"), "{killed,1000}");
}

/// A process that keeps all it makes, in a VM whose thread may hold only so much more than it
/// held when it started: from 1.25 MiB to 8 MiB, its heap meets the allocator's refusal wherever
/// it grows, at the copy a collection starts with (the fewest pages), halfway through that copy,
/// or keeping its headroom between instructions (the most).
#[test]
fn a_flood_past_the_pages_left_kills_its_process() {
    for quarters in 5..=32 {
        let mut vm = Vm::with_limits(Box::new(TestPlatform { now: 0 }), Limits::default());
        let pid = vm.spawn("allocation", "heap", |_| Vec::new()).unwrap();
        PAGES.with(|p| p.set(HELD.with(Cell::get) + quarters * MOST / 4));
        let r = vm.run_bounded(pid, 10_000_000).expect("finished");
        PAGES.with(|p| p.set(usize::MAX));
        assert_eq!(r.unwrap().unwrap().to_string(), "{killed,1000}", "{quarters} quarter MiB");
    }
}

#[test]
fn a_binary_built_past_the_allocator_kills_its_builder() {
    assert_eq!(run("binary"), "{killed,1000}");
}

#[test]
fn a_binary_grown_in_place_past_the_allocator_kills_its_builder() {
    assert_eq!(run("append"), "{killed,1000}");
}

#[test]
fn a_tuple_past_the_allocator_kills_its_maker() {
    assert_eq!(run("tuple"), "{killed,1000}");
}

#[test]
fn a_list_copy_past_the_allocator_kills_its_maker() {
    assert_eq!(run("list_copy"), "{killed,1000}");
}

#[test]
fn a_binarys_bytes_as_a_list_past_the_allocator_kill_its_maker() {
    assert_eq!(run("byte_list"), "{killed,1000}");
}

#[test]
fn a_binary_copied_past_the_allocator_kills_its_maker() {
    assert_eq!(run("copies"), "{killed,1000}");
}

#[test]
fn an_iolist_flattened_past_the_allocator_kills_its_maker() {
    assert_eq!(run("iolist"), "{killed,1000}");
}

#[test]
fn a_tuple_past_the_processs_own_limit_is_refused_before_it_is_made() {
    assert_eq!(run("limit"), "{killed,1000}");
}

#[test]
fn an_allocation_past_the_limit_only_with_garbage_is_made_after_a_collection() {
    assert_eq!(run("collected"), "{50000,1000}");
}

#[test]
fn a_maps_keys_past_the_allocator_kill_their_maker() {
    assert_eq!(run("map_keys"), "{killed,1000}");
}

#[test]
fn a_maps_values_past_the_allocator_kill_their_maker() {
    assert_eq!(run("map_values"), "{killed,1000}");
}

#[test]
fn a_maps_pairs_past_the_allocator_kill_their_maker() {
    assert_eq!(run("map_to_list"), "{killed,1000}");
}

#[test]
fn a_map_from_a_list_past_the_allocator_kills_its_maker() {
    assert_eq!(run("map_from_list"), "{killed,1000}");
}

#[test]
fn a_map_from_keys_past_the_allocator_kills_its_maker() {
    assert_eq!(run("map_from_keys"), "{killed,1000}");
}

#[test]
fn table_objects_looked_up_past_the_allocator_kill_the_reader() {
    assert_eq!(run("ets_lookup"), "{killed,1000}");
}

#[test]
fn table_objects_taken_past_the_allocator_kill_the_taker() {
    assert_eq!(run("ets_take"), "{killed,1000}");
}

#[test]
fn table_elements_looked_up_past_the_allocator_kill_the_reader() {
    assert_eq!(run("ets_lookup_element"), "{killed,1000}");
}

#[test]
fn a_tables_objects_matched_past_the_allocator_kill_the_reader() {
    assert_eq!(run("ets_match_object"), "{killed,1000}");
}

#[test]
fn characters_listed_past_the_allocator_kill_their_maker() {
    assert_eq!(run("characters"), "{killed,1000}");
}

#[test]
fn a_term_decoded_past_the_allocator_kills_its_decoder() {
    assert_eq!(run("decoded"), "{killed,1000}");
}

#[test]
fn dictionary_keys_past_the_allocator_kill_their_maker() {
    assert_eq!(run("dict_keys"), "{killed,1000}");
}

#[test]
fn a_dictionary_listed_past_the_allocator_kills_its_owner() {
    assert_eq!(run("dict_get"), "{killed,1000}");
}

#[test]
fn a_dictionary_erased_past_the_allocator_kills_its_owner() {
    assert_eq!(run("dict_erase"), "{killed,1000}");
}

#[test]
fn a_bignums_digits_past_the_allocator_kill_their_maker() {
    assert_eq!(run("integer_text"), "{killed,1000}");
}

/// The text of a bignum within the VM's 2^24 bits, in binary: 16 MiB of digits, asked for before
/// any is made. The bignum itself takes 2 MiB, so this test's allocator gives 4 MiB at once.
#[test]
fn a_bignums_binary_digits_past_the_allocator_kill_their_maker() {
    assert_eq!(run_capped("integer_bits", 4 << 20), "{killed,1000}");
}

#[test]
fn a_natives_copy_of_a_tables_elements_past_the_pages_left_kills_its_caller() {
    assert_eq!(run("held_elements"), "{killed,1000}");
}

#[test]
fn a_chunked_matchs_copy_past_the_pages_left_kills_its_caller() {
    assert_eq!(run("held_chunk"), "{killed,1000}");
}

#[test]
fn a_natives_copy_of_a_tuple_past_the_pages_left_kills_its_caller() {
    assert_eq!(run("held_tuple"), "{killed,1000}");
}

#[test]
fn a_natives_copy_of_a_list_past_the_pages_left_kills_its_caller() {
    assert_eq!(run("held"), "{killed,1000}");
}
