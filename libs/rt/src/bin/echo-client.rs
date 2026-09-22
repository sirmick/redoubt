//! The echo client: through the namespace its startup block gives it (`/` is the echo server),
//! writes to `/echo`, reads it back, and checks that `..` stays at the root and that a call
//! that is not 9P is refused. Exits 0 on success, or the number of the step that failed; prints
//! `ECHO TEST PASSED` or `ECHO TEST FAILED` on `/dev/cons` if it has one.
//!
//! Built only on `redoubt-rt`. On the host, `tests/echo.rs` runs it against a fake kernel.

#![cfg_attr(target_os = "none", no_std, no_main)]

use redoubt_rt::abi::{FOREVER, Handle, MAX_LEND_PAGES};
use redoubt_rt::client::Client;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::MALFORMED;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// What the client writes and expects back: longer than one page, so it crosses pages of the lend.
pub const MESSAGE: &[u8] = &[b'e'; 5000];

const ROOT: u32 = 0;
const FILE: u32 = 1;

/// Runs the checks; 0 if all passed, else the failed step's number.
pub fn run(startup: &Startup) -> u32 {
    let code = check(startup).err().unwrap_or(0);
    if let Some((_, console)) = startup.namespace().find(|(path, _)| *path == "/dev/cons") {
        let verdict: &[u8] = if code == 0 { b"ECHO TEST PASSED\n" } else { b"ECHO TEST FAILED\n" };
        let _ = print(console, verdict);
    }
    code
}

fn check(startup: &Startup) -> Result<(), u32> {
    // `/` is the echo server's connection, so `/echo` resolves to its file `echo`.
    let (server, rest) = startup.resolve("/echo").ok_or(10u32)?;
    if rest != "echo" {
        return Err(10);
    }
    let mut echo = Client::new(Endpoint::from_handle(server), MAX_LEND_PAGES).map_err(|_| 11u32)?;
    echo.version().map_err(|_| 12u32)?;
    echo.attach(ROOT, "").map_err(|_| 13u32)?;
    // `..` never climbs above the root: the client cleans it away, and the server would too.
    echo.walk(ROOT, FILE, "../../echo").map_err(|_| 14u32)?;
    echo.open(FILE, mode::ORDWR | mode::OTRUNC).map_err(|_| 15u32)?;
    let mut sent = 0;
    while sent < MESSAGE.len() {
        let n = echo.write(FILE, sent as u64, &MESSAGE[sent..]).map_err(|_| 16u32)?;
        if n == 0 {
            return Err(16);
        }
        sent += n;
    }
    let mut back = [0u8; 6000];
    let mut got = 0;
    loop {
        let n = echo.read(FILE, got as u64, &mut back[got..]).map_err(|_| 17u32)?;
        if n == 0 {
            break;
        }
        got += n;
    }
    if &back[..got] != MESSAGE {
        return Err(18);
    }
    echo.clunk(FILE).map_err(|_| 19u32)?;
    // A call with words that are not 9P's is malformed (status 1), not served.
    let endpoint = echo.into_endpoint();
    let (reply, _) = endpoint.call(&[1, 2, 3, 4], &[], None, FOREVER).into_result().map_err(|_| 20u32)?;
    if reply.words != MALFORMED {
        return Err(21);
    }
    Ok(())
}

fn print(console: Handle, text: &[u8]) -> Result<(), redoubt_rt::client::ClientError> {
    let mut cons = Client::new(Endpoint::from_handle(console), 1)?;
    cons.attach(ROOT, "")?;
    cons.open(ROOT, mode::OWRITE)?;
    cons.write(ROOT, 0, text)?;
    cons.clunk(ROOT)
}
