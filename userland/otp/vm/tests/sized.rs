//! A resource that declares its size counts as its holder's own memory, toward the process's own
//! `max_heap_size` (docs/userland/beamlet.md, "Screen natives": a buffer counts toward its
//! owner's heap limit). The test's natives make such resources; the fixture's source is
//! `src/sized.erl`.

use beamlet_vm::bif::{Ctx, NativeSpec};
use beamlet_vm::platform::{Lookup, Platform, PlatformError};
use beamlet_vm::vm::Config;
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
fn run(f: &str) -> String {
    let config = Config { natives: NATIVES, ..Default::default() };
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
