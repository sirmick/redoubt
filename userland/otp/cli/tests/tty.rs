//! beamlet on a terminal (docs/userland/beamlet.md, "The console on a host"): the tty is in raw
//! mode while a process of the VM reads the console, and has its own settings back whichever way
//! beamlet ends: a normal result, an exception, a halt, a signal. Each test opens a
//! pseudo-terminal, runs the binary with the slave end as its console, and compares the slave's
//! settings after the run with those before it. A panic is `src/tty.rs`'s own test, since the
//! binary has no way to be made to panic. The fixture's source is `src/tty_probe.erl`.

use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rustix::fd::{AsFd, OwnedFd};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{LocalModes, tcgetattr};

/// A pseudo-terminal: the master end, which the test types on and drains, and the slave end,
/// the terminal beamlet runs on.
fn pty() -> (OwnedFd, OwnedFd) {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let slave = std::fs::OpenOptions::new().read(true).write(true).open(name.to_str().unwrap()).unwrap();
    (master, slave.into())
}

/// The terminal's settings that raw mode changes, as four words of flags.
fn modes(fd: impl AsFd) -> (u32, u32, u32, u32) {
    let t = tcgetattr(fd).unwrap();
    (t.input_modes.bits(), t.output_modes.bits(), t.control_modes.bits(), t.local_modes.bits())
}

fn canonical(fd: impl AsFd) -> bool { tcgetattr(fd).unwrap().local_modes.contains(LocalModes::ICANON) }

/// Runs `tty_probe:FUNCTION()` on the slave end, draining what it writes so it never blocks.
fn beamlet(slave: &OwnedFd, master: &OwnedFd, function: &str) -> Child {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let mut drain = std::fs::File::from(master.try_clone().unwrap());
    std::thread::spawn(move || {
        let mut sink = [0u8; 4096];
        while drain.read(&mut sink).is_ok_and(|n| n > 0) {}
    });
    Command::new(env!("CARGO_BIN_EXE_beamlet"))
        .args(["-pa", fixtures, "tty_probe", function])
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()))
        .spawn()
        .unwrap()
}

/// Waits for the probe to read the console, which is when the terminal goes raw.
fn wait_raw(slave: &OwnedFd) {
    let start = Instant::now();
    while canonical(slave) {
        assert!(start.elapsed() < Duration::from_secs(20), "the terminal never went raw");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Types a line, so the probe's read ends and it goes on to its end.
fn type_line(master: &OwnedFd) {
    std::fs::File::from(master.try_clone().unwrap()).write_all(b"x\n").unwrap();
}

#[test]
fn the_terminal_is_raw_while_the_vm_reads_it_and_restored_at_a_normal_end() {
    let (master, slave) = pty();
    let before = modes(&slave);
    assert!(canonical(&slave));

    let mut child = beamlet(&slave, &master, "line");
    wait_raw(&slave);
    let raw = tcgetattr(&slave).unwrap();
    assert!(!raw.local_modes.contains(LocalModes::ECHO), "no echo in raw mode");
    assert!(!raw.local_modes.contains(LocalModes::ISIG), "Ctrl+C is a byte, not a signal");
    type_line(&master);

    assert!(child.wait().unwrap().success());
    assert_eq!(modes(&slave), before);
}

#[test]
fn the_terminal_is_restored_when_the_run_ends_in_an_exception() {
    let (master, slave) = pty();
    let before = modes(&slave);

    let mut child = beamlet(&slave, &master, "crash");
    wait_raw(&slave);
    type_line(&master);

    // An exception is a result beamlet prints, so the exit status is success.
    assert!(child.wait().unwrap().success());
    assert_eq!(modes(&slave), before);
}

#[test]
fn the_terminal_is_restored_when_the_vm_halts() {
    let (master, slave) = pty();
    let before = modes(&slave);

    let mut child = beamlet(&slave, &master, "halt");
    wait_raw(&slave);
    type_line(&master);

    assert_eq!(child.wait().unwrap().code(), Some(3));
    assert_eq!(modes(&slave), before);
}

#[test]
fn the_terminal_is_restored_when_a_signal_ends_beamlet() {
    let (master, slave) = pty();
    let before = modes(&slave);

    let mut child = beamlet(&slave, &master, "line");
    wait_raw(&slave);
    rustix::process::kill_process(rustix::process::Pid::from_child(&child), rustix::process::Signal::TERM)
        .unwrap();

    // The handler restores the terminal and lets the signal end the process as it would have.
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(libc_sigterm()));
    assert_eq!(modes(&slave), before);
}

fn libc_sigterm() -> i32 { rustix::process::Signal::TERM.as_raw() }
