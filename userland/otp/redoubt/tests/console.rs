//! The platform's console, over the client library's hub, against the fixture's console server, on
//! the fake kernel: what the VM writes reaches the screen, whole and in order, what is typed
//! reaches the VM, a read with nothing typed yet waits without holding the VM's thread, the end of
//! the input is the end, and the console costs one waiter thread and no other.

use std::io::Write;
use std::sync::{Arc, Mutex};

use beamlet_redoubt::Redoubt;
use beamlet_redoubt::fixture::{self, ConsoleServer, Dirs};
use beamlet_vm::platform::{ConsoleInput, Platform};
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
    let f = fake();
    let screen = Screen::default();
    let console: ConsoleServer = fixture::console(Box::new(input), Box::new(screen.clone()));
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
    bytes
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

#[test]
fn a_console_without_consol_has_no_size() {
    with_platform(std::io::empty(), |p| assert_eq!(p.console_size(), None));
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
