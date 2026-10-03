//! A launcher's end releases its orphan's connections (servers/wire.md, "A launcher releases its
//! child's grants"). In `init`'s place, since servers under `init` hold no budget handle (R33),
//! this tester launches, each through the real stub with the client library's `Launch` and each
//! in a budget it carved:
//! - `orphan-server`, a 9P server on an endpoint whose receive right it hands over, and whose founding
//!   connection the tester keeps;
//! - `orphan-child`, C, with a receive right of its own, in a budget beside the launcher's, so the launcher's
//!   end does not end it;
//! - `orphan-launcher`, L, with a connection the tester minted for it through its own and recorded in L's
//!   `Grants`, and a badge at C's endpoint.
//!
//! L mints C's connection through its own, hands it to C, and faults. The tester's `Job::wait`
//! releases L's grants, which disconnects L's connection at the server, and with it every one
//! minted under it. The verdict is the server's own count of the connections it holds, asked
//! before and after: both L's and C's are gone, though C still runs. C's word that its next
//! attach was refused is printed as a trace.

#![no_std]
#![no_main]

extern crate alloc;

use core::fmt::Write;
use core::num::NonZeroU64;

use redoubt_client::Lend;
use redoubt_client::file::Connection;
use redoubt_client::grants::Grants;
use redoubt_client::launch::{Job, Launch};
use redoubt_init_programs::orphan;
use redoubt_rt::abi::{BudgetSpec, Cause, FOREVER, Handle, Handles, Labels};
use redoubt_rt::handle::{Budget, Endpoint};
use redoubt_rt::ipc::{Event, Request};
use redoubt_rt::server::typed::{Outcome, finish};
use test_programs::bundle::Bundle;
use test_programs::console::{self, Console};
use test_programs::rd;

static STUB_BIN: &[u8] = include_bytes!(env!("STUB_BIN"));

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out, $($arg)*).ok(); }};
}

/// Each child's budget, in pages: its stub, its image, its stack and its tables.
const CHILD_PAGES: u64 = 256;

fn h(index: u32) -> Handle { Handle::new(index).expect("a slot is at least 1") }

fn badge(n: u64) -> NonZeroU64 { NonZeroU64::new(n).expect("a badge is not 0") }

#[no_mangle]
pub extern "C" fn _start(bundle: usize, len: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    console::init(uart);
    let mut out = Console;
    say!(out, "\n[launcher-orphan] starting");
    // SAFETY: the loader mapped the whole verified bundle here, read-only, `len` bytes from
    // `bundle`, for as long as this program runs (kernel/boot.md).
    let bundle = unsafe { Bundle::at(bundle, len) };
    match bundle.ok_or("no bundle").and_then(|b| run(&mut out, b)) {
        Ok(()) => say!(out, "[launcher-orphan] LAUNCHER ORPHAN TEST PASSED"),
        Err(why) => say!(out, "[launcher-orphan] FAIL: {why}"),
    }
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

/// A budget for one child, carved from `system`, as `init` carves a server's.
fn carve() -> Result<Budget, &'static str> {
    let spec = BudgetSpec {
        pages: CHILD_PAGES,
        processes: 1,
        weight: 100,
        labels: Labels::from_slice(&[]).map_err(|_| "an empty label set")?,
        account: 0,
        deadline: FOREVER,
    };
    Budget::from_handle(h(rd::SYSTEM)).create_child(&spec).map_err(|_| "carve a child's budget")
}

/// The bundle entry `name`: a program the case packs.
fn image(bundle: Bundle, name: &str) -> Result<&'static [u8], &'static str> {
    bundle.find(name.as_bytes()).map(|e| e.data).ok_or("a program missing from the bundle")
}

/// `orphan-server`'s count of the connections it minted and has not disconnected.
fn count(server: &Endpoint) -> Result<u64, &'static str> {
    let reply = server.call(&[orphan::COUNT, 0, 0, 0], &[], None, FOREVER).into_result();
    let (reply, _) = reply.map_err(|_| "ask the server its count")?;
    if reply.words[0] != orphan::OK[0] {
        return Err("the server refused its count");
    }
    Ok(reply.words[1])
}

fn run(out: &mut Console, bundle: Bundle) -> Result<(), &'static str> {
    let mut lend = Lend::new(1).map_err(|_| "map a lend")?;
    let endpoint = || Endpoint::create().map_err(|_| "create an endpoint");

    // The server, and the tester's own founding connection to it.
    let serving = endpoint()?;
    let mut launch = Launch::new(STUB_BIN, image(bundle, "orphan-server")?, carve()?, endpoint()?);
    launch.handle("orphan-server", serving.handle());
    let _server = launch.start().map_err(|_| "start the server")?;
    let server = serving.mint(badge(1), None).map_err(|_| "mint the tester's badge")?;
    let own = Connection::attach(Endpoint::from_handle(server.handle()), &mut lend)
        .map_err(|_| "attach to the server")?;

    // C, with its own endpoint, and L's way to it and to the tester.
    let reports = endpoint()?;
    let child_at = endpoint()?;
    let mut launch = Launch::new(STUB_BIN, image(bundle, "orphan-child")?, carve()?, endpoint()?);
    let to_tester = reports.mint(badge(2), None).map_err(|_| "mint C's badge at the tester")?;
    launch.handle("launcher", child_at.handle()).handle("tester", to_tester.handle());
    let _child = launch.start().map_err(|_| "start C")?;

    let mut grants = Grants::new();
    let granted = grants.connection(&mut lend, &own, "", 0).map_err(|_| "mint L's connection")?;
    let to_child = child_at.mint(badge(3), None).map_err(|_| "mint L's badge at C")?;
    let to_tester = reports.mint(badge(4), None).map_err(|_| "mint L's badge at the tester")?;
    let mut launch = Launch::new(STUB_BIN, image(bundle, "orphan-launcher")?, carve()?, endpoint()?);
    launch
        .handle("server", granted.handle())
        .handle("child", to_child.handle())
        .handle("tester", to_tester.handle())
        .grants(grants);
    let mut launcher: Job = launch.start().map_err(|_| "start L")?;

    // L's call once C holds its connection, and C's word that it attached through it.
    let (mut minted, mut attached) = (None::<Request>, None);
    while minted.is_none() || attached.is_none() {
        match reports.receive(FOREVER, 0) {
            Ok(Event::Call(request)) if request.words[0] == orphan::MINTED => minted = Some(request),
            Ok(Event::Send(d)) if d.words[0] == orphan::ATTACHED => attached = Some(d.words[1] == 1),
            Ok(_) => {}
            Err(_) => return Err("receive the reports"),
        }
    }
    if attached != Some(true) {
        return Err("C could not attach through the connection L minted for it");
    }
    let before = count(&server)?;
    say!(out, "[launcher-orphan] ok: L minted C's connection under its own; the server holds {before}");

    // L faults when answered; its end's notice releases its grants.
    let request = minted.expect("received above");
    let none = Handles::new();
    let _ = finish(request, &Outcome { words: orphan::OK, send: none, close: none });
    let ended = launcher.wait(FOREVER).map_err(|_| "wait for L's end")?;
    if ended.notice.cause != Cause::Faulted {
        return Err("L did not end by its fault");
    }
    ended.released.map_err(|_| "release L's grants")?;
    let after = count(&server)?;
    say!(out, "[launcher-orphan] ok: L faulted and its grants were released; the server holds {after}");
    if before < 2 || after != before - 2 {
        return Err("the server still holds L's connection or C's");
    }

    // C runs on, its connection dead.
    let again = child_at.mint(badge(5), None).map_err(|_| "mint the tester's badge at C")?;
    again.send(&[orphan::AGAIN, 0, 0, 0], &[], None, FOREVER).map_err(|_| "tell C to try again")?;
    let refused = loop {
        match reports.receive(FOREVER, 0) {
            Ok(Event::Send(d)) if d.words[0] == orphan::TRIED => break d.words[1] == 1,
            Ok(_) => {}
            Err(_) => return Err("receive C's word"),
        }
    };
    let said = if refused { "refused" } else { "accepted" };
    say!(out, "[launcher-orphan] trace: C, still running, says its next attach was {said}");
    Ok(())
}
