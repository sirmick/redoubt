//! Resource limits: a process that exceeds one is ended the way BEAM would end it, and nothing
//! else is disturbed. BEAM has no limits for some of these, so these are not differential tests.
//! The fixture's source is `src/limits.erl`.

use std::collections::BTreeMap;

use beamlet_vm::platform::{Platform, PlatformError};
use beamlet_vm::vm::Limits;
use beamlet_vm::Vm;

struct TestPlatform {
    now: u64,
}

impl Platform for TestPlatform {
    fn monotonic_us(&mut self) -> u64 {
        self.now += 1;
        self.now
    }
    fn system_time_us(&mut self) -> Option<u64> {
        None
    }
    fn idle(&mut self, deadline: Option<u64>) {
        if let Some(d) = deadline {
            self.now = self.now.max(d);
        }
    }
    fn console_write(&mut self, _bytes: &[u8]) {}
    fn random(&mut self, _buf: &mut [u8]) -> Result<(), PlatformError> {
        Err(PlatformError::Unavailable)
    }
    fn load_module(&mut self, module: &str) -> Option<Vec<u8>> {
        let modules: BTreeMap<&str, &[u8]> =
            [("limits", include_bytes!("fixtures/limits.beam").as_slice())].into_iter().collect();
        modules.get(module).map(|b| b.to_vec())
    }
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
fn programs_need_the_platform_to_grant_them() {
    assert_eq!(run("no_programs", small()), "{error,eacces}");
}
