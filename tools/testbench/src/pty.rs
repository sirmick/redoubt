//! The terminal a `pty = true` session's `ssh` reads its input from (docs/testbench.md, "SSH
//! sessions"). OpenSSH sends a `window-change` only when its standard input is a terminal whose size
//! changed, so the runner gives that session a pseudo-terminal as its standard input; its output stays
//! on the pipes the runner reads. The terminal is the client's input, never a verdict.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::OpenOptionsExt;

/// A new pseudo-terminal of `cols` by `rows`: its master, for the runner, and its slave, for `ssh`.
/// Both are opened close-on-exec, as `std` opens every file, so no other child the bench starts
/// inherits a session's terminal; `ssh` gets its slave as its standard input only.
pub fn open(cols: u16, rows: u16) -> io::Result<(File, File)> {
    let master = OpenOptions::new().read(true).write(true).custom_flags(libc::O_NOCTTY).open("/dev/ptmx")?;
    let fd = master.as_raw_fd();
    let (unlock, size) =
        (0 as libc::c_int, libc::winsize { ws_col: cols, ws_row: rows, ws_xpixel: 0, ws_ypixel: 0 });
    // SAFETY: `fd` is the open master for as long as `master` lives; `TIOCSPTLCK` and `TIOCSWINSZ`
    // only read the `c_int` and the `winsize` they are given.
    if unsafe {
        libc::ioctl(fd, libc::TIOCSPTLCK, &unlock) != 0 || libc::ioctl(fd, libc::TIOCSWINSZ, &size) != 0
    } {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `TIOCGPTPEER` takes open flags by value and touches no memory of ours.
    let slave =
        unsafe { libc::ioctl(fd, libc::TIOCGPTPEER, libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    if slave < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `TIOCGPTPEER` succeeded, so `slave` is a new open descriptor that nothing else owns.
    Ok((master, unsafe { File::from_raw_fd(slave) }))
}

/// Sets the terminal's size to `cols` by `rows` and tells `pid`, which reads from it, with
/// `SIGWINCH`: the terminal is no process's controlling terminal, so the kernel tells no one.
/// `pid` must be a child not yet waited for, so that it names no other process.
pub fn resize(master: &File, pid: u32, cols: u16, rows: u16) -> io::Result<()> {
    let size = libc::winsize { ws_col: cols, ws_row: rows, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: `master` is an open terminal descriptor for as long as the borrow, and `TIOCSWINSZ`
    // only reads the `winsize` it is given.
    if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let pid = libc::pid_t::try_from(pid).map_err(io::Error::other)?;
    // SAFETY: `kill` touches no memory of ours; the caller has not waited for `pid`, so it is still
    // that child's, and `SIGWINCH` only asks it to read the terminal's size again.
    if unsafe { libc::kill(pid, libc::SIGWINCH) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;

    /// Whether the kernel holds `fd` close-on-exec, from its `flags:` line in `/proc`.
    fn close_on_exec(fd: &impl AsRawFd) -> bool {
        let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", fd.as_raw_fd())).unwrap();
        let flags = info.lines().find_map(|l| l.strip_prefix("flags:")).unwrap().trim();
        u32::from_str_radix(flags, 8).unwrap() & libc::O_CLOEXEC as u32 != 0
    }

    #[test]
    fn no_other_child_inherits_a_sessions_terminal() {
        let (master, slave) = super::open(80, 24).unwrap();
        assert!(close_on_exec(&master));
        assert!(close_on_exec(&slave));
    }
}
