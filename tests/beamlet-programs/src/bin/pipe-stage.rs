//! `pipe-stage`: the native program the pipe cases run as a session's stages (docs/userland/native.md,
//! "Standard input and output, and pipes"). It has what a stage has, `/dev/stdin`, `/dev/stdout`
//! and `/dev/stderr`, and its first argument says what it does with them:
//!
//! - `cat`: copies its input to its output, to the end of the input.
//! - `count`: reads its input to the end, then writes how many bytes it held, in decimal.
//! - `head N`: reads N bytes at most, writes them, and exits without reading more.
//! - `say WORDS...`: writes the words and a newline, reading nothing.
//! - `gen N`: writes N bytes, a line of `x`s at a time, reading nothing.
//! - `yes`: writes `y` lines until a write is refused, then exits [`REFUSED`].
//! - `exit`: exits at once, reading nothing.
//! - `wait`: reads nothing and never ends.
//! - `hostile`: writes control sequences to its output and its standard error.
//! - `attack`: tries every way past its three streams (below), then writes `attack-out` and a newline: its
//!   output is the only thing it can reach, so that is all the next stage gets.
//!
//! What `attack` tries, each of which must fail: any other name in its namespace (`/`, `/boot`,
//! `/home/alice`, `/dev/cons`, `/dev/pipe`) and any named handle; its input opened for writing, by
//! any path or through any connection minted from it, and its output for reading; a walk across
//! from its output to another end, another pipe or the `canary` pipe, opened or minted; a file made
//! beside them; and a call on every handle number up to 64 that is not one of its own. Each write
//! it tries carries `pwned`, which therefore reaches nothing. It says nothing about how they went: its report
//! would be the attacker's word (docs/testbench.md, rule F). The session judges what the attempts reached.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use redoubt_client::file::{Connection, File};
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend};
use redoubt_rt::abi::Handle;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::{DMDIR, mode};
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::proto::ninep_common;

redoubt_rt::entry!(run);

/// No lend, or its namespace did not attach.
const NO_STREAM: u32 = 2;
/// Its input, output or standard error is not there or does not open: 6, 7 or 8.
const NO_STDIN: u32 = 6;
/// A write the pipe refused, as `yes` ends.
const REFUSED: u32 = 4;
/// An argument it does not know.
const BAD_ARGS: u32 = 5;

const LEND_PAGES: usize = 2;
const CHUNK: usize = 2048;

/// Its three streams, each open its own way.
struct Streams {
    lend: Lend,
    stdin: File,
    stdout: File,
    stderr: File,
}

impl Streams {
    /// The three, or the exit code that names the first that would not open.
    fn open(ns: &Namespace, mut lend: Lend) -> Result<Streams, u32> {
        let mut stream = |i: u32, path: &str, how: u8| -> Result<File, u32> {
            let (conn, rest) = ns.lookup(path).ok_or(NO_STDIN + i)?;
            conn.open(&mut lend, rest, how).map_err(|_| NO_STDIN + i)
        };
        let stdin = stream(0, "/dev/stdin", mode::OREAD)?;
        let stdout = stream(1, "/dev/stdout", mode::OWRITE)?;
        let stderr = stream(2, "/dev/stderr", mode::OWRITE)?;
        Ok(Streams { lend, stdin, stdout, stderr })
    }

    /// What one read gives: empty at the end of the input.
    fn read(&mut self, max: usize) -> Result<Vec<u8>, Error> {
        let mut buf = vec![0; max.min(CHUNK)];
        let n = self.stdin.read_at(&mut self.lend, 0, &mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Writes all of `bytes` to `out`, however many writes the pipe takes.
    fn write(lend: &mut Lend, out: &File, mut bytes: &[u8]) -> Result<(), Error> {
        while !bytes.is_empty() {
            match out.write_at(lend, 0, &bytes[..bytes.len().min(CHUNK)])? {
                0 => return Err(Error::Unexpected),
                n => bytes = &bytes[n..],
            }
        }
        Ok(())
    }

    fn out(&mut self, bytes: &[u8]) -> Result<(), Error> {
        Streams::write(&mut self.lend, &self.stdout, bytes)
    }

    fn err(&mut self, bytes: &[u8]) -> Result<(), Error> {
        Streams::write(&mut self.lend, &self.stderr, bytes)
    }
}

fn run(startup: &Startup) -> u32 {
    let Ok(mut lend) = Lend::new(LEND_PAGES) else { return NO_STREAM };
    let Ok(ns) = Namespace::from_startup(startup, &mut lend) else { return NO_STREAM };
    let args: Vec<&str> = startup.args().collect();
    if args.first() == Some(&"attack") {
        attack(startup, &ns, &mut lend);
    }
    let mut s = match Streams::open(&ns, lend) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let done = match args.as_slice() {
        ["cat"] => cat(&mut s),
        ["count"] => count(&mut s),
        ["head", n] => n.parse().map_or(Ok(BAD_ARGS), |n| head(&mut s, n)),
        ["say", words @ ..] => s.out(format!("{}\n", words.join(" ")).as_bytes()).map(|()| 0),
        ["gen", n] => n.parse().map_or(Ok(BAD_ARGS), |n| generate(&mut s, n)),
        ["yes"] => Ok(yes(&mut s)),
        ["exit"] => Ok(0),
        ["wait"] => redoubt_init_programs::park(),
        ["hostile"] => hostile(&mut s),
        ["attack"] => s.out(b"attack-out\n").map(|()| 0),
        _ => Ok(BAD_ARGS),
    };
    done.unwrap_or(NO_STREAM)
}

fn cat(s: &mut Streams) -> Result<u32, Error> {
    loop {
        let data = s.read(CHUNK)?;
        if data.is_empty() {
            return Ok(0);
        }
        s.out(&data)?;
    }
}

fn count(s: &mut Streams) -> Result<u32, Error> {
    let mut n = 0usize;
    loop {
        let data = s.read(CHUNK)?;
        if data.is_empty() {
            s.out(format!("{n}\n").as_bytes())?;
            return Ok(0);
        }
        n += data.len();
    }
}

fn head(s: &mut Streams, mut n: usize) -> Result<u32, Error> {
    while n > 0 {
        let data = s.read(n)?;
        if data.is_empty() {
            break;
        }
        n -= data.len();
        s.out(&data)?;
    }
    Ok(0)
}

fn generate(s: &mut Streams, mut n: usize) -> Result<u32, Error> {
    let line = [b"x".repeat(63), b"\n".to_vec()].concat();
    while n > 0 {
        let take = n.min(line.len());
        s.out(&line[line.len() - take..])?;
        n -= take;
    }
    Ok(0)
}

fn yes(s: &mut Streams) -> u32 {
    loop {
        if s.out(b"y\n").is_err() {
            return REFUSED;
        }
    }
}

/// A clipboard write, a hyperlink, a title, a title report, a bare ESC and DEL: every one must
/// reach the console as visible text.
const HOSTILE: &[u8] =
    b"\x1b]52;c;aGk=\x07\x1b]8;;http://x\x07link\x1b]8;;\x07\x1b]0;pwned\x07\x1b[21t\x1bX\x7f\n";

fn hostile(s: &mut Streams) -> Result<u32, Error> {
    s.out(HOSTILE)?;
    s.err(HOSTILE)?;
    Ok(0)
}

/// Every way past the three streams it can try; none may reach anything. Nothing is reported.
fn attack(startup: &Startup, ns: &Namespace, lend: &mut Lend) {
    for path in ["/", "/boot", "/home/alice", "/dev/cons", "/dev/pipe", "/dev/pipe/canary/w"] {
        if let Some((conn, rest)) = ns.lookup(path) {
            let _ = conn.open(lend, rest, mode::OWRITE).and_then(|f| f.write_at(lend, 0, b"pwned"));
        }
    }
    for (_, handle) in startup.handles() {
        let _ = Endpoint::from_handle(handle).send(&[0, 0, 0, 0], &[], None, 1000);
    }
    let lookup = |path: &str| ns.lookup(path).map(|(c, _)| c.clone());
    let (Some(stdin), Some(stdout)) = (lookup("/dev/stdin"), lookup("/dev/stdout")) else { return };
    // Its input is a read end: nothing reached from it, or minted through it, takes a write.
    for path in ["", "..", "../w", "../r", "w", "../../canary/w", "/canary/w"] {
        let _ = stdin.open(lend, path, mode::OWRITE).and_then(|f| f.write_at(lend, 0, b"pwned"));
        let _ = stdin.open(lend, path, mode::ORDWR).and_then(|f| f.write_at(lend, 0, b"pwned"));
        if let Ok((endpoint, _)) = stdin.new_connection(lend, path, 0) {
            if let Ok(minted) = Connection::attach(endpoint, lend) {
                let _ = minted.open(lend, "", mode::OWRITE).and_then(|f| f.write_at(lend, 0, b"pwned"));
            }
        }
    }
    // Its output is its own to write, `..` and a mint at its root included, and nothing else:
    // every path across to another end or pipe, opened or minted, must fail.
    let _ = stdout.open(lend, "", mode::OREAD).map(|f| f.read_at(lend, 0, &mut [0; 16]));
    for path in ["../r", "r", "../../canary/w", "../../canary/r", "/canary/w", "canary/w"] {
        let _ = stdout.open(lend, path, mode::OWRITE).and_then(|f| f.write_at(lend, 0, b"pwned"));
        if let Ok((endpoint, _)) = stdout.new_connection(lend, path, 0) {
            if let Ok(minted) = Connection::attach(endpoint, lend) {
                let _ = minted.open(lend, "", mode::OWRITE).and_then(|f| f.write_at(lend, 0, b"pwned"));
            }
        }
    }
    for conn in [&stdin, &stdout] {
        let _ = conn.create(lend, "", "pwned", DMDIR | 0o700, mode::OREAD);
        let _ = conn.create(lend, "", "pwned", 0o600, mode::OWRITE);
    }
    // Handles it was never given: the kernel's table holds nothing at them.
    let held: Vec<u32> = ns.list().map(|(_, c)| c.endpoint().handle().index()).collect();
    let mut words = [0u64; 4];
    let mut body = [0u8; 64];
    let request =
        ninep_common::Message::NewConnection(ninep_common::NewConnection { root: "canary/w", quota: 0 });
    if let Ok(w) = request.encode(&mut body) {
        words = w;
    }
    for index in 1..=64u32 {
        let Some(handle) = Handle::new(index).filter(|h| !held.contains(&h.index())) else { continue };
        let _ = Endpoint::from_handle(handle).call(&words, &[], None, 1_000_000);
    }
}
