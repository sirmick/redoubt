//! Who reads the console (docs/userland/beamlet.md, "The console on a host"): one process at a
//! time. While it lives, another's subscription is refused and the input keeps going to it, so
//! no code run at the shell's prompt can take the keyboard, or the interrupt key with it, from
//! the shell's driver; once it has exited, the next may subscribe. The fixture's source is
//! `src/console.erl`.

use beamlet_vm::Vm;
use beamlet_vm::platform::{ConsoleInput, Lookup, Platform, PlatformError};

/// A console with one line typed on it, whose size is known on the first call and gone on the
/// second, that changes size once, and a clock that jumps to each deadline it idles to.
struct Typed {
    now: u64,
    input: Option<Vec<u8>>,
    size: Option<(u16, u16)>,
    resized: Option<(u16, u16)>,
}

impl Platform for Typed {
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

    fn console_read(&mut self) -> ConsoleInput {
        match self.input.take() {
            Some(bytes) => ConsoleInput::Data(bytes),
            None => ConsoleInput::Nothing,
        }
    }

    fn console_size(&mut self) -> Option<(u16, u16)> { self.size.take() }

    fn console_resized(&mut self) -> Option<(u16, u16)> { self.resized.take() }

    fn random(&mut self, _buf: &mut [u8]) -> Result<(), PlatformError> { Err(PlatformError::Unavailable) }

    fn load_module(&mut self, module: &str) -> Lookup {
        match module {
            "console" => Lookup::Found(include_bytes!("fixtures/console.beam").to_vec()),
            _ => Lookup::Absent,
        }
    }
}

/// Runs `console:f()` and returns its result as text.
fn run(f: &str) -> String {
    let typed = Typed { now: 0, input: Some(b"x".to_vec()), size: Some((40, 12)), resized: Some((132, 43)) };
    let mut vm = Vm::new(Box::new(typed));
    let pid = vm.spawn("console", f, |_| Vec::new()).unwrap();
    vm.run_bounded(pid, 10_000_000).expect("finished").unwrap().unwrap().to_string()
}

#[test]
fn the_console_size_is_the_platforms_answer_asked_at_each_call() {
    assert_eq!(run("size"), "{{40,12},unknown}");
}

#[test]
fn a_change_of_the_console_s_size_reaches_its_reader_once() {
    assert_eq!(run("resize"), "{{132,43},<<120>>}");
}

#[test]
fn a_second_console_subscription_is_refused_and_the_first_reader_keeps_the_console() {
    assert_eq!(run("second_reader"), "{{error,busy},<<120>>,none}");
}

#[test]
fn the_console_is_free_once_its_reader_has_exited() {
    assert_eq!(run("after_exit"), "ok");
}
