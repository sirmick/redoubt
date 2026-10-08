//! The shell on a real terminal: beamlet's binary on a pseudo-terminal, running Redoubt's shell,
//! and a screen in front of it (docs/userland/shell.md, "Full-screen programs"). It needs the
//! shell built and the code path ./test-shell gives it in `SHELL_PTY_ARGS` (beamlet's arguments,
//! one a line), so cargo runs it only when asked: `./test-shell` does.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustix::fd::{AsFd, OwnedFd};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{Winsize, tcgetattr, tcsetwinsize};

fn pty() -> (OwnedFd, OwnedFd) {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let slave = std::fs::OpenOptions::new().read(true).write(true).open(name.to_str().unwrap()).unwrap();
    (master, slave.into())
}

fn modes(fd: impl AsFd) -> (u32, u32, u32, u32) {
    let t = tcgetattr(fd).unwrap();
    (t.input_modes.bits(), t.output_modes.bits(), t.control_modes.bits(), t.local_modes.bits())
}

/// The shell on the slave end, and everything it writes, gathered as it comes.
struct Session {
    child: Child,
    master: std::fs::File,
    seen: Arc<Mutex<Vec<u8>>>,
}

impl Session {
    fn start(slave: &OwnedFd, master: &OwnedFd) -> Session {
        let args = std::env::var("SHELL_PTY_ARGS").expect("SHELL_PTY_ARGS: run by ./test-shell");
        let args: Vec<&str> = args.lines().filter(|l| !l.is_empty()).collect();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut drain = std::fs::File::from(master.try_clone().unwrap());
        let gathered = Arc::clone(&seen);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = drain.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                gathered.lock().unwrap().extend_from_slice(&chunk[..n]);
            }
        });
        let child = Command::new(env!("CARGO_BIN_EXE_beamlet"))
            .args(&args)
            .arg("Elixir.Redoubt.Shell")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave.try_clone().unwrap()))
            .spawn()
            .unwrap();
        Session { child, master: std::fs::File::from(master.try_clone().unwrap()), seen }
    }

    fn typed(&mut self, bytes: &[u8]) { self.master.write_all(bytes).unwrap(); }

    /// Waits until `what` has been written after byte `from`, and returns where it ends.
    fn wait_for(&self, what: &[u8], from: usize) -> usize {
        let start = Instant::now();
        loop {
            {
                let seen = self.seen.lock().unwrap();
                if let Some(at) = seen[from.min(seen.len())..].windows(what.len()).position(|w| w == what) {
                    return from + at + what.len();
                }
            }
            if start.elapsed() > Duration::from_secs(120) {
                let seen = self.seen.lock().unwrap();
                panic!(
                    "never saw {:?}; saw {:?}",
                    String::from_utf8_lossy(what),
                    String::from_utf8_lossy(&seen)
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
#[ignore = "needs the shell built: ./test-shell runs it"]
fn pick_on_a_terminal_takes_the_screen_and_gives_it_back_with_the_choice() {
    let (master, slave) = pty();
    tcsetwinsize(&slave, Winsize { ws_row: 20, ws_col: 70, ws_xpixel: 0, ws_ypixel: 0 }).unwrap();
    let before = modes(&slave);
    let mut s = Session::start(&slave, &master);

    let at = s.wait_for(b"(1)> ", 0);
    s.typed(b"pick([\"apple\", \"banana\", \"cherry\"])\r");
    let at = s.wait_for(b"\x1b[?1049h", at);
    let at = s.wait_for(b"cherry", at);
    // Down, then Enter: the second.
    s.typed(b"\x1b[B");
    s.typed(b"\r");
    let at = s.wait_for(b"\x1b[?1049l", at);
    let at = s.wait_for(b"\"banana\"", at);
    s.wait_for(b"(2)> ", at);

    s.typed(b"exit\r");
    let status = s.child.wait().unwrap();
    assert!(status.success(), "{status:?}");
    assert_eq!(modes(&slave), before, "the terminal's settings are back");
}
