//! The VM's limits from its budget (docs/userland/beamlet.md, "Limits inside one VM"): one
//! process's heap and all ETS tables each get a sixteenth of the budget, in machine words, so a
//! flood meets its limit in Erlang before the budget ends the VM; `budget_pages=N` gives the
//! budget, and `run` builds its VM with these limits.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use beamlet_redoubt::fixture::{self, ConsoleServer, Dirs};
use beamlet_redoubt::{budget_pages, limits, run};
use beamlet_vm::vm::Limits;
use redoubt_fake_kernel::fake;

#[test]
fn a_sixteenth_of_the_budget_goes_to_each_limit() {
    let word = core::mem::size_of::<usize>() as u64;
    let got = limits(Some(4096));
    let sixteenth = 4096 * 4096 / 16 / word;
    assert_eq!(got.max_heap_words, sixteenth);
    assert_eq!(got.max_ets_words, sixteenth);
    // The rest are the VM's own.
    assert_eq!(got.max_mailbox, Limits::default().max_mailbox);
}

#[test]
fn without_a_budget_the_defaults_stand() {
    let (got, default) = (limits(None), Limits::default());
    assert_eq!((got.max_heap_words, got.max_ets_words), (default.max_heap_words, default.max_ets_words));
}

#[test]
fn a_huge_budget_saturates_rather_than_wraps() {
    assert_eq!(limits(Some(u64::MAX)).max_heap_words, u64::MAX / 16 / core::mem::size_of::<usize>() as u64);
}

#[test]
fn the_budget_is_one_decimal_argument_anywhere() {
    assert_eq!(budget_pages(["budget_pages=4096", "m"].into_iter()), Some(4096));
    assert_eq!(budget_pages(["m", "f", "budget_pages=7"].into_iter()), Some(7));
}

#[test]
fn a_missing_or_malformed_budget_is_none() {
    for args in [
        &[][..],
        &["m", "f"],
        &["budget_pages="],
        &["budget_pages=0"],
        &["budget_pages=-1"],
        &["budget_pages=+5"],
        &["budget_pages=4k"],
        &["budget_pages= 4"],
        &["budget_pages=99999999999999999999999"],
        &["budget_pages=4", "budget_pages=4"],
    ] {
        assert_eq!(budget_pages(args.iter().copied()), None, "{args:?}");
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

/// Runs `limits:function()`, the VM's own limits fixture, through `run` with a budget of
/// `budget_pages`, and returns its exit code and what reached the console.
fn run_limits(function: &'static str, budget_pages: u64) -> (u32, String) {
    let f = fake();
    let screen = Screen::default();
    let console: ConsoleServer = fixture::console(Box::new(std::io::empty()), Box::new(screen.clone()));
    let (pid, block) = fixture::session(&console);
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vm/tests/fixtures");
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        run(&startup, Box::new(Dirs(vec![fixtures])), "limits", function, Some(budget_pages), None)
    });
    let code = session.join().unwrap();
    f.destroy(console.pid, console.endpoint);
    assert_eq!(console.thread.join().unwrap(), redoubt_rt::exit::OK);
    let text = String::from_utf8(screen.0.lock().unwrap().clone()).unwrap();
    (code, text)
}

#[test]
fn run_kills_a_process_at_a_sixteenth_of_the_budget() {
    // 64 pages: a heap limit of 16 KiB in words, which a list of 2^30 cells passes long before.
    assert_eq!(run_limits("heap", 64), (0, "killed\n".to_string()));
}

#[test]
fn run_refuses_tables_past_a_sixteenth_of_the_budget() {
    assert_eq!(run_limits("ets", 64), (0, "{system_limit,true}\n".to_string()));
}
