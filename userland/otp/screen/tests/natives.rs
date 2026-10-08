//! The screen buffer's natives in the VM (docs/userland/beamlet.md, "Screen natives"): what a
//! screen program can and cannot make them do. The fixture's source is `src/screen_probe.erl`.

use beamlet_vm::platform::{Lookup, Platform, PlatformError};
use beamlet_vm::term::OwnedTerm;
use beamlet_vm::vm::Config;
use beamlet_vm::{Term, Vm};

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
            "screen_probe" => Lookup::Found(include_bytes!("fixtures/screen_probe.beam").to_vec()),
            _ => Lookup::Absent,
        }
    }
}

/// Runs `screen_probe:f()` and returns its result.
fn run(f: &str) -> OwnedTerm {
    let config = Config { natives: beamlet_screen::NATIVES, ..Default::default() };
    let mut vm = Vm::with_config(Box::new(Bare { now: 0 }), config);
    let pid = vm.spawn("screen_probe", f, |_| Vec::new()).unwrap();
    vm.run_bounded(pid, 100_000_000).expect("finished").unwrap().unwrap()
}

/// The elements of a tuple result.
fn elements(t: &OwnedTerm) -> Vec<Term> { t.heap().as_tuple(t.term()).expect("a tuple").to_vec() }

/// A binary's bytes, decoded as a frame by the decoder the session uses.
fn frame(t: &OwnedTerm, at: Term) -> cells::Frame {
    let bytes = t.heap().as_bits(at).expect("a binary").to_bytes().into_owned();
    cells::decode(&bytes).expect("a frame the session reads")
}

#[test]
fn drawing_gives_a_frame_of_the_cells_drawn_then_one_of_nothing() {
    let result = run("drawn");
    let e = elements(&result);
    assert_eq!(e[0].as_i64(), Some(2), "two columns written");
    let first = frame(&result, e[1]);
    assert!(first.clear);
    let cells: Vec<(u16, u16, &str)> = first.cells.iter().map(|c| (c.x, c.y, c.symbol.as_str())).collect();
    assert_eq!(cells, [(1, 0, "h"), (2, 0, "i"), (0, 1, "-"), (1, 1, "-"), (2, 1, "-"), (9, 1, "\u{28ff}")]);
    assert_eq!(first.cells[0].fg, cells::Color::Indexed(2));
    assert_eq!(first.cells[0].modifiers.bits(), 1);
    let second = frame(&result, e[2]);
    assert!(!second.clear);
    assert!(second.cells.is_empty());
}

#[test]
fn a_control_character_is_badarg_and_nothing_is_drawn() {
    let result = run("control");
    assert!(result.to_string().starts_with("{badarg,badarg,"), "{result}");
    let e = elements(&result);
    assert!(frame(&result, e[2]).cells.is_empty());
}

#[test]
fn only_the_process_that_made_a_buffer_draws_into_it() {
    assert_eq!(run("other_owner").to_string(), "badarg");
}

#[test]
fn a_process_holds_at_most_four_buffers() {
    assert_eq!(run("fifth").to_string(), "{4,system_limit}");
}

#[test]
fn a_buffer_nothing_holds_is_given_back() {
    assert_eq!(run("freed").to_string(), "true");
}

#[test]
fn a_large_buffer_ends_its_owner_at_its_heap_limit() {
    assert_eq!(run("big").to_string(), "killed");
}

#[test]
fn a_buffer_resized_past_the_heap_limit_ends_its_owner() {
    assert_eq!(run("grown").to_string(), "killed");
}

#[test]
fn a_style_that_is_not_one_is_badarg() {
    assert_eq!(run("styles").to_string(), "[badarg,badarg,badarg,badarg]");
}
