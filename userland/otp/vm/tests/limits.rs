//! Resource limits: a process that exceeds one is ended the way BEAM would end it, and nothing
//! else is disturbed. BEAM has no limits for some of these, so these are not differential tests.
//! The fixture's source is `src/limits.erl`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use beamlet_vm::Vm;
use beamlet_vm::platform::{ConsoleInput, Lookup, Platform, PlatformError};
use beamlet_vm::vm::{Config, Limits};

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

    fn console_write(&mut self, _bytes: &[u8]) {}

    fn random(&mut self, _buf: &mut [u8]) -> Result<(), PlatformError> { Err(PlatformError::Unavailable) }

    fn load_module(&mut self, module: &str) -> Lookup {
        let modules: BTreeMap<&str, &[u8]> =
            [("limits", include_bytes!("fixtures/limits.beam").as_slice())].into_iter().collect();
        modules.get(module).map_or(Lookup::Absent, |b| Lookup::Found(b.to_vec()))
    }
}

/// A console with nothing to read until the VM first waits, and then its end, which keeps what is
/// written.
struct Console {
    inner: TestPlatform,
    waited: bool,
    written: Arc<Mutex<Vec<u8>>>,
}

impl Platform for Console {
    fn monotonic_us(&mut self) -> u64 { self.inner.monotonic_us() }

    fn system_time_us(&mut self) -> Option<u64> { None }

    fn idle(&mut self, deadline: Option<u64>) {
        self.waited = true;
        self.inner.idle(deadline)
    }

    fn console_write(&mut self, bytes: &[u8]) { self.written.lock().unwrap().extend_from_slice(bytes) }

    fn console_read(&mut self) -> ConsoleInput {
        if self.waited { ConsoleInput::Eof } else { ConsoleInput::Nothing }
    }

    fn random(&mut self, buf: &mut [u8]) -> Result<(), PlatformError> { self.inner.random(buf) }

    fn load_module(&mut self, module: &str) -> Lookup { self.inner.load_module(module) }
}

/// Runs `limits:footprint()`, which waits for console input, and returns what the console got.
fn footprint(report_memory: Option<beamlet_vm::memory::HeapPages>) -> String {
    let written = Arc::new(Mutex::new(Vec::new()));
    let platform = Console { inner: TestPlatform { now: 0 }, waited: false, written: Arc::clone(&written) };
    let mut vm = Vm::with_config(Box::new(platform), Config { report_memory, ..Config::default() });
    let pid = vm.spawn("limits", "footprint", |_| Vec::new()).unwrap();
    let r = vm.run_bounded(pid, 1_000_000).expect("finished");
    assert_eq!(r.unwrap().unwrap().to_string(), "eof");
    let report = written.lock().unwrap().clone();
    String::from_utf8(report).unwrap()
}

/// Run `limits:f()` under `limits` and return its result as text.
fn run(f: &str, limits: Limits) -> String {
    let mut vm = Vm::with_limits(Box::new(TestPlatform { now: 0 }), limits);
    let pid = vm.spawn("limits", f, |_| Vec::new()).unwrap();
    let r = vm.run_bounded(pid, 1_000_000).expect("finished");
    match r.unwrap() {
        Ok(t) => t.to_string(),
        Err(e) => format!("raised {}", e.reason),
    }
}

fn small() -> Limits {
    Limits { max_mailbox: 100, max_heap_words: 1 << 20, max_ets_words: 1 << 16, ..Limits::default() }
}

#[test]
fn full_mailbox_kills_the_receiver() {
    assert_eq!(run("mailbox", small()), "{system_limit,message_queue}");
}

#[test]
fn full_own_mailbox_kills_the_sender() {
    assert_eq!(run("mailbox_self", small()), "{system_limit,message_queue}");
}

#[test]
fn a_roomy_mailbox_is_not_a_limit() {
    let limits = Limits { max_mailbox: 10_000, ..small() };
    assert_eq!(run("mailbox", limits), "alive");
}

#[test]
fn the_vm_heap_limit_kills() {
    assert_eq!(run("heap", small()), "killed");
}

#[test]
fn a_process_can_lower_its_own_limit() {
    assert_eq!(run("heap_flag", Limits::default()), "killed");
}

#[test]
fn spawn_opt_sets_a_limit() {
    assert_eq!(run("heap_spawn_opt", Limits::default()), "killed");
}

#[test]
fn under_the_limit_nothing_happens() {
    assert_eq!(run("heap_ok", small()), "100");
}

#[test]
fn ets_inserts_past_the_limit_raise() {
    assert_eq!(run("ets", small()), "{system_limit,true}");
}

#[test]
fn memory_is_reported() {
    assert_eq!(run("memory", small()), "{true,true,true}");
}

#[test]
fn the_footprint_is_reported_at_the_first_wait_for_input() {
    let report = footprint(Some(|| Some((7, 9))));
    for row in
        ["sizes Instr=", "code.instrs count=", "code.operands count=", "literal_table count=", "atoms count="]
    {
        assert!(report.contains(&format!("footprint {row}")), "no {row} in {report}");
    }
    assert!(report.contains(" replaced=0 largest "), "{report}");
    assert!(report.contains("footprint process.heaps_collected count="), "{report}");
    assert!(report.contains("footprint total held="), "{report}");
    assert!(
        report.ends_with("footprint runtime pages held_now=7 peak=9 unaccounted=0 transient=2\n"),
        "{report}"
    );
    assert_eq!(report.matches("footprint sizes").count(), 1, "reported once: {report}");
    assert_eq!(footprint(None), "", "silent unless asked");
}

#[test]
fn held_bytes_follow_the_runtime_heap() {
    use beamlet_vm::memory::held;
    assert_eq!(
        [held(0), held(1), held(16), held(17), held(2048), held(2049), held(8192)],
        [0, 16, 16, 32, 2048, 4096, 8192]
    );
}

#[test]
fn programs_need_the_platform_to_grant_them() {
    assert_eq!(run("no_programs", small()), "{error,eacces}");
}

#[test]
fn garbage_is_collected_and_live_data_survives() {
    assert_eq!(run("garbage", Limits::default()), "{true,1000}");
}

#[test]
fn unreferenced_binaries_are_freed() {
    assert_eq!(run("binary_garbage", Limits::default()), "true");
}
