//! The platform's console, over the client library's hub, against the fixture's console server, on
//! the fake kernel: what the VM writes reaches the screen, whole and in order, what is typed
//! reaches the VM, a read with nothing typed yet waits without holding the VM's thread, the end of
//! the input is the end, a VM away from it past the server's session bound keeps it, a write
//! answered `busy` goes again after the retry interval, and the console costs one waiter thread
//! and no other.

use std::io::Write;
use std::sync::{Arc, Mutex};

use beamlet_redoubt::Redoubt;
use beamlet_redoubt::fixture::{self, ConsoleServer, Dirs};
use beamlet_vm::platform::{ConsoleInput, Platform};
use redoubt_client::aio::RETRY_US;
use redoubt_fake_kernel::fake;

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

/// Runs `test` on a platform in a session of a console server that reads `input`, then stops
/// the server, and returns what reached its screen.
fn with_platform(
    input: impl std::io::Read + Send + 'static,
    test: impl FnOnce(&mut Redoubt) + Send + 'static,
) -> Vec<u8> {
    with_platform_on(input, |input, screen| fixture::console(input, screen), test).0
}

/// As [`with_platform`], on the console server `start` makes of the input and the screen; also
/// returns when each write reached the server.
fn with_platform_on(
    input: impl std::io::Read + Send + 'static,
    start: impl FnOnce(Box<dyn std::io::Read + Send>, Box<dyn Write + Send>) -> ConsoleServer,
    test: impl FnOnce(&mut Redoubt) + Send + 'static,
) -> (Vec<u8>, Vec<u64>) {
    let f = fake();
    let screen = Screen::default();
    let console = start(Box::new(input), Box::new(screen.clone()));
    let (pid, block) = fixture::session(&console);
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        let mut platform = Redoubt::new(&startup, Box::new(Dirs(Vec::new()))).expect("a platform");
        test(&mut platform);
        0
    });
    assert_eq!(session.join().unwrap(), 0);
    f.destroy(console.pid, console.endpoint);
    assert_eq!(console.thread.join().unwrap(), redoubt_rt::exit::OK);
    let bytes = screen.0.lock().unwrap().clone();
    let writes = console.writes.lock().unwrap().clone();
    (bytes, writes)
}

/// Everything read until the end of the input, idling between reads as the VM does.
fn read_to_end(platform: &mut Redoubt) -> Vec<u8> {
    let mut read = Vec::new();
    loop {
        match platform.console_read() {
            ConsoleInput::Data(bytes) => read.extend(bytes),
            ConsoleInput::Eof => return read,
            ConsoleInput::Nothing => platform.idle(None),
        }
    }
}

#[test]
fn writes_reach_the_screen() {
    let screen = with_platform(std::io::empty(), |p| p.console_write(b"hello, console\r\n"));
    assert_eq!(screen, b"hello, console\r\n");
}

#[test]
fn a_long_write_reaches_the_screen_whole_and_in_order() {
    // Several of the hub's writes, a page each, one out at a time.
    let long: Vec<u8> = (0..20_000u32).map(|i| b'a' + (i % 26) as u8).collect();
    let expected = long.clone();
    let screen = with_platform(std::io::empty(), move |p| {
        p.console_write(&long[..7]);
        p.console_write(&long[7..]);
    });
    assert_eq!(screen, expected);
}

#[test]
fn the_console_is_one_hub_connection_with_one_waiter() {
    let (keyboard, mut keys) = std::io::pipe().unwrap();
    with_platform(keyboard, move |p| {
        // The waiter is the console connection's, started with the platform: no thread per read.
        assert_eq!(p.waiters(), 1);
        assert_eq!(p.console_read(), ConsoleInput::Nothing);
        p.console_write(b"> ");
        keys.write_all(b"x\n").unwrap();
        drop(keys);
        assert_eq!(read_to_end(p), b"x\n");
        assert_eq!(p.waiters(), 1);
    });
}

#[test]
fn typing_reaches_the_vm_then_its_end() {
    // Longer than one of the fixture's device reads, so it comes in pieces.
    let typed = b"Enum.map([1, 2, 3], &(&1 * 10))\n1 + 2\n".to_vec();
    let expected = typed.clone();
    with_platform(std::io::Cursor::new(typed), move |p| assert_eq!(read_to_end(p), expected));
}

#[test]
fn a_read_waits_for_typing_without_holding_the_vm() {
    let (keyboard, mut keys) = std::io::pipe().unwrap();
    with_platform(keyboard, move |p| {
        // Nothing typed: the read answers at once, and the VM's thread goes on.
        assert_eq!(p.console_read(), ConsoleInput::Nothing);
        p.console_write(b"> ");
        keys.write_all(b"ls\n").unwrap();
        // The typing wakes the idle VM.
        let mut read = Vec::new();
        while read != b"ls\n" {
            p.idle(None);
            if let ConsoleInput::Data(bytes) = p.console_read() {
                read.extend(bytes);
            }
        }
        // The keyboard goes away: the input ends.
        drop(keys);
        assert_eq!(read_to_end(p), b"");
    });
}

/// The reader leaves with typing queued: with nobody to take it, it is no reason to wake the VM,
/// whose idle sleeps to its deadline rather than returning at once on every pass; the next reader
/// gets what was held, then what is typed after.
#[test]
fn input_nobody_reads_holds_no_idle_and_waits_for_the_next_reader() {
    let (keyboard, mut keys) = std::io::pipe().unwrap();
    with_platform(keyboard, move |p| {
        assert_eq!(p.console_read(), ConsoleInput::Nothing);
        // One key, so it arrives in one piece, and is left queued: the reader goes away before
        // taking it.
        keys.write_all(b"y").unwrap();
        p.idle(None);
        p.console_listening(false);
        let deadline = p.monotonic_us() + 200_000;
        p.idle(Some(deadline));
        assert!(p.monotonic_us() >= deadline, "the idle returned on input nobody reads");
        // A reader again: what was held first, then the rest.
        p.console_listening(true);
        assert_eq!(p.console_read(), ConsoleInput::Data(b"y".to_vec()));
        keys.write_all(b"later\n").unwrap();
        let mut read = Vec::new();
        while read != b"later\n" {
            p.idle(None);
            if let ConsoleInput::Data(bytes) = p.console_read() {
                read.extend(bytes);
            }
        }
        drop(keys);
        assert_eq!(read_to_end(p), b"");
    });
}

#[test]
fn a_vm_busy_past_the_session_bound_keeps_its_console() {
    let (keyboard, mut keys) = std::io::pipe().unwrap();
    with_platform(keyboard, move |p| {
        // A read parked at the server and a write's answer on its way, then the VM's thread is
        // away from the platform longer than the console server's session bound.
        assert_eq!(p.console_read(), ConsoleInput::Nothing);
        p.console_write(b"> ");
        std::thread::sleep(std::time::Duration::from_secs(12));
        keys.write_all(b"ls\n").unwrap();
        let mut read = Vec::new();
        while read != b"ls\n" {
            match p.console_read() {
                ConsoleInput::Data(bytes) => read.extend(bytes),
                ConsoleInput::Nothing => p.idle(None),
                ConsoleInput::Eof => panic!("the console ended while the VM was busy"),
            }
        }
        drop(keys);
        assert_eq!(read_to_end(p), b"");
    });
}

/// A console over its share answers a write `busy`; the write goes again `RETRY_US` later, as the
/// hub's rule for a queued request has it, not at once: a console that stays busy costs the VM
/// a request every retry interval, never a spin.
#[test]
fn a_write_answered_busy_goes_again_after_the_retry_interval() {
    let (screen, writes) = with_platform_on(
        std::io::empty(),
        |i, o| fixture::console_busy(i, o, 2),
        |p| {
            p.console_write(b"third time lucky\r\n");
            // Idle past the two retries, as the VM does with nothing to run.
            let until = p.monotonic_us() + 10 * RETRY_US;
            while p.monotonic_us() < until {
                p.idle(Some(until));
            }
        },
    );
    assert_eq!(screen, b"third time lucky\r\n");
    assert_eq!(writes.len(), 3, "two refused, then taken: {writes:?}");
    for pair in writes.windows(2) {
        assert!(pair[1] - pair[0] >= RETRY_US, "a retry went early: {writes:?}");
    }
}

/// The input's end that reaches the VM's endpoint before the VM idles is handed over by that idle,
/// as input is: the VM is not left waiting until its deadline, or for ever while it serves, with
/// the end already taken. The end is held back until the first read has gone out and come back
/// empty: an input already at its end could be answered before that read looks.
#[test]
fn an_end_of_input_already_waiting_ends_the_idle_that_takes_it() {
    let (keyboard, keys) = std::io::pipe().unwrap();
    with_platform(keyboard, move |p| {
        // The first read goes out and is parked: nothing is typed.
        assert_eq!(p.console_read(), ConsoleInput::Nothing);
        // The keyboard goes away: the read's answer is the end, queued at the VM's endpoint while
        // the VM is away.
        drop(keys);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let deadline = p.monotonic_us() + 10_000_000;
        p.idle(Some(deadline));
        assert!(
            deadline.saturating_sub(p.monotonic_us()) > 5_000_000,
            "the idle slept on the end it had taken"
        );
        assert_eq!(p.console_read(), ConsoleInput::Eof);
        // Read once, the end is no longer news: an idle after it sleeps to its deadline.
        let deadline = p.monotonic_us() + 20_000;
        p.idle(Some(deadline));
        assert!(p.monotonic_us() >= deadline);
    });
}

#[test]
fn the_console_s_size_is_its_server_s() {
    with_platform(std::io::empty(), |p| assert_eq!(p.console_size(), Some((80, 24))));
}

/// Once reading has begun, the platform keeps a `resize` out at the console's server, an SSH
/// channel's, whose share holds it beside the session's completion call; a change of the window's
/// size reaches the VM through `console_resized`, once, and an idle returns for it.
#[test]
fn a_change_of_the_console_s_size_reaches_the_vm_once_reading_has_begun() {
    let f = fake();
    let (keyboard, keys) = std::io::pipe().unwrap();
    let console = fixture::console_channel(Box::new(keyboard), Box::new(Screen::default()));
    let (pid, block) = fixture::session(&console);
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        let mut p = Redoubt::new(&startup, Box::new(Dirs(Vec::new()))).expect("a platform");
        assert_eq!(p.console_resized(), None, "no change before reading");
        assert_eq!(p.console_read(), ConsoleInput::Nothing);
        let (cols, rows) = loop {
            p.idle(None);
            if let Some(size) = p.console_resized() {
                break size;
            }
        };
        assert_eq!(p.console_resized(), None, "a change is taken once");
        u32::from(cols) << 16 | u32::from(rows)
    });
    // A change counts only once the platform's `resize` is parked at the server.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while console.resizes_waiting() != 1 {
        assert!(std::time::Instant::now() < until, "the resize was never parked");
        std::thread::yield_now();
    }
    console.resize(132, 43);
    assert_eq!(session.join().unwrap(), 132 << 16 | 43);
    drop(keys);
    f.destroy(console.pid, console.endpoint);
    assert_eq!(console.thread.join().unwrap(), redoubt_rt::exit::OK);
}

#[test]
fn idling_with_a_deadline_returns_by_it() {
    with_platform(std::io::empty(), |p| {
        let deadline = p.monotonic_us() + 20_000;
        p.idle(Some(deadline));
        assert!(p.monotonic_us() >= deadline);
    });
}

#[test]
fn after_the_console_ends_idling_still_waits_for_its_deadline() {
    with_platform(std::io::empty(), |p| {
        assert_eq!(read_to_end(p), b"");
        // A timer after the end, a logger's or a sleep's: the VM sleeps until it, not spinning.
        let deadline = p.monotonic_us() + 20_000;
        p.idle(Some(deadline));
        assert!(p.monotonic_us() >= deadline);
    });
}

#[test]
fn there_is_no_wall_clock() { with_platform(std::io::empty(), |p| assert_eq!(p.system_time_us(), None)); }
