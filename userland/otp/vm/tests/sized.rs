//! A resource that declares its size counts as its holder's own memory, toward the process's own
//! `max_heap_size` (docs/userland/beamlet.md, "Limits inside one VM"). The test's natives make
//! such resources, and so do the VM's own: an atomics or counters array declares its cells, and a
//! zlib stream its queues and its codec's state. Past the limit, each ends its holder as heap
//! growth does. The fixture's source is `src/sized.erl`.

use beamlet_vm::bif::{Ctx, NativeSpec};
use beamlet_vm::platform::{Lookup, Platform, PlatformError};
use beamlet_vm::vm::{Config, Limits};
use beamlet_vm::{Exception, Term, Vm};

/// `probe:sized(Bytes)`: a resource declaring `Bytes`.
fn sized(c: &mut Ctx, a: &[Term]) -> Result<Term, Exception> {
    let bytes = a[0].as_i64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| c.badarg())?;
    Ok(c.new_resource_sized(0u8, bytes))
}

/// `probe:resize(Resource, Bytes)`: the resource now declares `Bytes`.
fn resize(c: &mut Ctx, a: &[Term]) -> Result<Term, Exception> {
    let bytes = a[1].as_i64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| c.badarg())?;
    c.resize_resource(a[0], bytes);
    Ok(c.ok())
}

static NATIVES: &[NativeSpec] = &[("probe", "sized", 1, sized), ("probe", "resize", 2, resize)];

/// A platform with a clock that jumps to each deadline it idles to, and the fixture.
struct Bare {
    now: u64,
}

impl Platform for Bare {
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

    fn console_write(&mut self, _bytes: &[u8]) {}

    fn random(&mut self, _buf: &mut [u8]) -> Result<(), PlatformError> { Err(PlatformError::Unavailable) }

    fn load_module(&mut self, module: &str) -> Lookup {
        match module {
            "sized" => Lookup::Found(include_bytes!("fixtures/sized.beam").to_vec()),
            _ => Lookup::Absent,
        }
    }
}

/// Runs `sized:f()` and returns its result as text.
fn run(f: &str) -> String { run_with(f, Limits::default()) }

/// Runs `sized:f()` with ETS and persistent_term each limited to 2^20 words (8 MB).
fn run_stores_limited(f: &str) -> String {
    run_with(f, Limits { max_ets_words: 1 << 20, max_persistent_words: 1 << 20, ..Limits::default() })
}

fn run_with(f: &str, limits: Limits) -> String {
    let config = Config { natives: NATIVES, limits, ..Default::default() };
    let mut vm = Vm::with_config(Box::new(Bare { now: 0 }), config);
    let pid = vm.spawn("sized", f, |_| Vec::new()).unwrap();
    vm.run_bounded(pid, 100_000_000).expect("finished").unwrap().unwrap().to_string()
}

#[test]
fn small_sized_resources_within_the_limit_are_held() {
    assert_eq!(run("small"), "normal");
}

#[test]
fn sized_resources_past_a_processs_own_heap_limit_end_it() {
    assert_eq!(run("many"), "killed");
}

#[test]
fn a_resource_resized_past_the_limit_ends_its_holder() {
    assert_eq!(run("grown"), "killed");
}

#[test]
fn an_atomics_array_within_the_limit_is_held() {
    assert_eq!(run("atomics_held"), "normal");
}

#[test]
fn an_atomics_array_past_a_processs_own_heap_limit_ends_it() {
    assert_eq!(run("atomics_past"), "killed");
}

#[test]
fn counters_arrays_past_the_limit_together_end_their_holder() {
    assert_eq!(run("counters_past"), "killed");
}

#[test]
fn a_zlib_stream_within_the_limit_is_held() {
    assert_eq!(run("zlib_held"), "normal");
}

#[test]
fn a_zlib_streams_queue_past_the_limit_ends_its_holder() {
    assert_eq!(run("zlib_queued_past"), "killed");
}

#[test]
fn zlib_codecs_past_the_limit_together_end_their_holder() {
    assert_eq!(run("zlib_codecs_past"), "killed");
}

#[test]
fn a_zlib_streams_stash_past_the_limit_ends_its_holder() {
    assert_eq!(run("zlib_stash_past"), "killed");
}

#[test]
fn a_128_mb_atomics_array_in_persistent_term_past_the_limit_is_refused() {
    assert_eq!(run_stores_limited("pt_atomics_past"), "{system_limit,none}");
}

#[test]
fn a_replaced_persistent_value_still_counts() {
    assert_eq!(run_stores_limited("pt_replaced"), "10");
}

#[test]
fn a_zlib_stream_grown_in_persistent_term_past_the_limit_refuses_the_next_put() {
    assert_eq!(run_stores_limited("pt_zlib_grown"), "system_limit");
}

#[test]
fn a_zlib_stream_grown_in_ets_past_the_limit_refuses_the_next_insert() {
    assert_eq!(run_stores_limited("ets_zlib_grown"), "{system_limit,true}");
}

#[test]
fn deleting_the_table_holding_a_grown_stream_makes_room() {
    assert_eq!(run_stores_limited("ets_table_deleted"), "{system_limit,true}");
}

#[test]
fn an_update_element_past_the_ets_limit_is_refused() {
    assert_eq!(run_stores_limited("ets_update_element_past"), "{system_limit,[{k,none}]}");
}

#[test]
fn heir_data_past_the_ets_limit_is_refused() {
    assert_eq!(run_stores_limited("ets_heir_past"), "{system_limit,system_limit}");
}

#[test]
fn heir_data_counts_toward_the_ets_limit() {
    assert_eq!(run_stores_limited("ets_heir_counts"), "{system_limit,true}");
}
