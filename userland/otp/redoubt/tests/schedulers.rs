//! A VM's schedulers on Redoubt (docs/userland/beamlet.md, "beamlet on Redoubt"): `schedulers=N`
//! gives `run`'s VM N scheduler threads, which on the fake kernel are host threads taking the
//! runtime's locks for real; work that crosses them through every path the system lock guards
//! loses nothing; and an absent or malformed count is one.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use beamlet_redoubt::fixture::{self, ConsoleServer, Dirs};
use beamlet_redoubt::{MAX_SCHEDULERS, run, schedulers};
use redoubt_fake_kernel::fake;

#[test]
fn the_count_is_one_decimal_argument_capped_and_one_otherwise() {
    assert_eq!(schedulers(["schedulers=2", "m"].into_iter()), 2);
    assert_eq!(schedulers(["m", "schedulers=8"].into_iter()), 8);
    assert_eq!(schedulers(["schedulers=64"].into_iter()), MAX_SCHEDULERS);
    for args in [
        &[][..],
        &["m", "f"],
        &["schedulers="],
        &["schedulers=0"],
        &["schedulers=-1"],
        &["schedulers=+2"],
        &["schedulers=2k"],
        &["schedulers= 2"],
        &["schedulers=99999999999999999999999"],
        &["schedulers=2", "schedulers=2"],
    ] {
        assert_eq!(schedulers(args.iter().copied()), 1, "{args:?}");
    }
}

/// The console server's screen, which a test reads back.
#[derive(Clone, Default)]
struct Screen(Arc<Mutex<Vec<u8>>>);

impl Write for Screen {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

/// Runs `schedulers:busy()` through `run` with `args`, and returns its exit code and what reached
/// the console.
fn run_busy(args: &[&str]) -> (u32, String) { run_fn("busy", args) }

/// Runs `schedulers:function()` through `run` with `args`, and returns its exit code and what
/// reached the console.
fn run_fn(function: &'static str, args: &[&str]) -> (u32, String) {
    let f = fake();
    let screen = Screen::default();
    let console: ConsoleServer = fixture::console(Box::new(std::io::empty()), Box::new(screen.clone()));
    let (pid, block) = fixture::session_with(&console, &[], args);
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vm/tests/fixtures");
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        run(&startup, Box::new(Dirs(vec![fixtures])), "schedulers", function, Some(4096), None)
    });
    let code = session.join().unwrap();
    f.destroy(console.pid, console.endpoint);
    assert_eq!(console.thread.join().unwrap(), redoubt_rt::exit::OK);
    let text = String::from_utf8(screen.0.lock().unwrap().clone()).unwrap();
    (code, text)
}

/// The fixture's answer with `n` schedulers: every sum right, every round trip made, every
/// insert kept, the timer fired.
fn answer(n: usize) -> String { format!("{{{n},8,[0,0,0,0,0,0,0,0],[2000,2000,2000,2000],400,tick}}\n") }

#[test]
fn two_schedulers_lose_nothing_across_the_system_lock() {
    // Several runs: each a new race between the two threads.
    for _ in 0..5 {
        assert_eq!(run_busy(&["schedulers=2"]), (0, answer(2)));
    }
}

#[test]
fn four_schedulers_lose_nothing_either() {
    assert_eq!(run_busy(&["schedulers=4"]), (0, answer(4)));
}

#[test]
fn without_the_argument_the_vm_has_one_scheduler() {
    assert_eq!(run_busy(&[]), (0, answer(1)));
}

#[test]
fn the_io_report_counts_the_schedulers() {
    let (code, text) = run_busy(&["schedulers=2", "report_io"]);
    assert_eq!(code, 0);
    assert!(text.contains("; threads: 2 schedulers, "), "{text}");
    let (_, one) = run_busy(&["report_io"]);
    assert!(one.contains("; threads: 1 scheduler, "), "{one}");
}

/// Rounds with one scheduler online and then two, a sleep before each: the helper parks and is
/// woken again each round, and every round finishes.
#[test]
fn schedulers_go_offline_and_online_again() {
    for _ in 0..5 {
        assert_eq!(run_fn("online", &["schedulers=2"]), (0, "{2,20}\n".to_string()));
    }
}
