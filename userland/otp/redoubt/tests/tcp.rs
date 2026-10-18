//! `gen_tcp` over `/net` (docs/userland/beamlet.md, "beamlet on Redoubt"): the Erlang backend
//! runs in a VM on the fake kernel against a scripted `/net` served as `ipd` serves its tree, and
//! the test reads what the script was asked (clone, ctl, data) and what the VM printed. OTP's
//! `prim_file`, `inet` and the standard library come from the pinned toolchain, the wire codec
//! from `libs/wire/elixir`, compiled here once: a host without the toolchain fails, naming it.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use beamlet_redoubt::fixture::{self, Dirs, NET_WAIT_US, NetScript, NetSeen, NetServer};
use beamlet_redoubt::run;
use redoubt_fake_kernel::fake;

/// A test's turn: the tests run one at a time, each with the one `/net` of its own. The clock is
/// the host's: OTP's `kernel` starts its logger when the VM boots, on timers a held clock would
/// never fire, and the one wait a test takes is the server's own, in real time.
fn turn() -> MutexGuard<'static, ()> {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
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

/// The pinned toolchain: `BEAMLET_TOOLCHAINS`, or the checkout's `toolchains` (tools/env.sh).
fn toolchains() -> PathBuf {
    std::env::var_os("BEAMLET_TOOLCHAINS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../toolchains"))
}

/// The one directory under `dir` whose name starts with `prefix` (`kernel-`), or a panic naming the lack.
fn one_dir(dir: &std::path::Path, prefix: &str) -> PathBuf {
    let found = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("the pinned toolchain at {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(prefix)));
    found.unwrap_or_else(|| panic!("no {prefix}* under {}", dir.display()))
}

/// The wire codec the backend frames `ctl` operations with, compiled from `libs/wire/elixir` with
/// the pinned `elixirc` into this workspace's target directory, once for the binary, through
/// `scripts/q run` like every build.
fn codec() -> PathBuf {
    static CODEC: OnceLock<PathBuf> = OnceLock::new();
    CODEC
        .get_or_init(|| {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
            let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/wire-codec");
            std::fs::create_dir_all(&out).expect("the codec's directory");
            let toolchains = toolchains();
            let (otp, elixir) = (one_dir(&toolchains, "otp-"), one_dir(&toolchains, "elixir-"));
            let path = format!(
                "{}:{}:{}",
                otp.join("bin").display(),
                elixir.join("bin").display(),
                std::env::var("PATH").unwrap_or_default()
            );
            let status = Command::new(root.join("scripts/q"))
                .env("PATH", path)
                .args(["run", "--cores", "1", "--"])
                .arg(elixir.join("bin/elixirc"))
                .arg("-o")
                .arg(&out)
                .arg(root.join("libs/wire/elixir/lib/wire.ex"))
                .arg(root.join("libs/wire/elixir/proto/net_ctl.ex"))
                .status()
                .expect("elixirc from the pinned toolchain, through scripts/q");
            assert!(status.success(), "elixirc failed: {status}");
            out
        })
        .clone()
}

/// The module directories the VM runs with: OTP's `erts` (the preloaded modules, `prim_file`
/// among them, ahead of the fixtures, whose `prim_file` is the VM tests' miniature), `kernel`
/// and `stdlib`, Elixir's standard library, the codec and the fixtures.
fn dirs() -> Vec<PathBuf> {
    let toolchains = toolchains();
    let otp_lib = one_dir(&toolchains, "otp-").join("lib");
    let elixir = one_dir(&toolchains, "elixir-").join("lib/elixir/ebin");
    vec![
        one_dir(&otp_lib, "erts-").join("ebin"),
        one_dir(&otp_lib, "kernel-").join("ebin"),
        one_dir(&otp_lib, "stdlib-").join("ebin"),
        elixir,
        codec(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vm/tests/fixtures"),
    ]
}

/// A VM running `tcp_backend:function()`, with `/dev/cons` and, when given, `/net`.
struct Running {
    session: std::thread::JoinHandle<u32>,
    console: fixture::ConsoleServer,
    screen: Screen,
}

fn start(function: &'static str, net: Option<&NetServer>) -> Running {
    let f = fake();
    let screen = Screen::default();
    let console = fixture::console(Box::new(std::io::empty()), Box::new(screen.clone()));
    let net = net.map(|n| (n.pid, n.endpoint));
    let (pid, block) = fixture::session_built(&console, &[], &[], &[], move |pid| {
        net.map_or_else(Vec::new, |(server, endpoint)| vec![("/net", f.grant(server, endpoint, pid, 0x70))])
    });
    let dirs = dirs();
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        run(&startup, Box::new(Dirs(dirs)), "tcp_backend", function, Some(4096), None)
    });
    Running { session, console, screen }
}

/// Waits for the VM to end and returns what it printed: the function's value.
fn finish(running: Running) -> String {
    assert_eq!(running.session.join().unwrap(), 0);
    fake().destroy(running.console.pid, running.console.endpoint);
    assert_eq!(running.console.thread.join().unwrap(), redoubt_rt::exit::OK);
    String::from_utf8(running.screen.0.lock().unwrap().clone()).unwrap()
}

fn stop(net: NetServer) {
    fake().destroy(net.pid, net.endpoint);
    let _ = net.thread.join();
}

/// Waits, on the host's clock, until `what` holds of the script.
fn until(net: &NetServer, why: &str, mut what: impl FnMut(&NetScript) -> bool) {
    let start = Instant::now();
    loop {
        let seen = {
            let script = net.script.lock().unwrap();
            if what(&script) {
                return;
            }
            script.seen.clone()
        };
        assert!(start.elapsed() < Duration::from_secs(60), "waited a minute for {why}: {seen:?}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The bench's peer, which the script reaches.
const PEER: ([u8; 4], u16) = ([10, 0, 9, 100], 7);
/// The box's own addresses, refused before any stack.
const OWN: [[u8; 4]; 2] = [[127, 0, 0, 1], [10, 0, 2, 15]];

/// A binary as the VM prints it.
fn text(s: &str) -> String {
    let bytes: Vec<String> = s.bytes().map(|b| b.to_string()).collect();
    format!("<<{}>>", bytes.join(","))
}

/// Connecting reads `clone`, writes `connect(addr, port)` in the wire encoding to the socket's
/// ctl and reads its state; bytes go out through `data` and come back through it; a half close
/// is `ctl close`, and the peer's end is 0 bytes, `{error, closed}` to the reader.
#[test]
fn a_connect_is_clone_ctl_and_data_and_the_peers_end_is_closed() {
    let _turn = turn();
    let net = fixture::net(&[PEER], &OWN, true);
    let running = start("echo", Some(&net));
    until(&net, "the half close", |s| s.seen.contains(&NetSeen::Close(1)));
    net.script.lock().unwrap().end(1);
    net.wake();
    let out = finish(running);
    let seen = net.script.lock().unwrap().seen.clone();
    assert_eq!(
        seen,
        [
            NetSeen::Clone(1),
            NetSeen::Connect(1, PEER.0, PEER.1),
            NetSeen::Wrote(1, b"hello".to_vec()),
            NetSeen::Wrote(1, b"again".to_vec()),
            NetSeen::Close(1),
        ]
    );
    assert!(
        out.contains(&format!(
            "{{{},{},{{{{10,0,9,100}},7}},{{error,closed}}}}",
            text("hello"),
            text("again")
        )),
        "{out}"
    );
    stop(net);
}

/// Active mode: what the peer sends arrives as `tcp` messages, framed in Erlang (line packets
/// across what one read brought), and its end as `tcp_closed`.
#[test]
fn active_mode_delivers_what_arrives_and_the_end() {
    let _turn = turn();
    let net = fixture::net(&[PEER], &[], false);
    let running = start("active", Some(&net));
    until(&net, "the connect", |s| s.seen.contains(&NetSeen::Connect(1, PEER.0, PEER.1)));
    until(&net, "the reader's read", |s| !s.holds(1).is_empty());
    {
        let mut script = net.script.lock().unwrap();
        script.feed(1, b"one\ntwo\n");
        script.end(1);
    }
    net.wake();
    let out = finish(running);
    assert!(out.contains(&format!("{{{},{},closed}}", text("one\n"), text("two\n"))), "{out}");
    stop(net);
}

/// A quiet peer: the server holds the read for its wait and answers `timeout`, the reader asks
/// again, and what the peer then sends reaches the recv whole.
#[test]
fn a_read_the_server_times_out_is_asked_again() {
    let _turn = turn();
    let net = fixture::net(&[PEER], &[], false);
    let running = start("quiet", Some(&net));
    until(&net, "the reader's read", |s| !s.holds(1).is_empty());
    let first = net.script.lock().unwrap().holds(1)[0];
    std::thread::sleep(Duration::from_micros(NET_WAIT_US + 200_000));
    // The server answered the first read `timeout` at its deadline, so a read held after it
    // is the reader's next one.
    until(&net, "the read asked again", |s| s.holds(1).iter().any(|t| *t > first + NET_WAIT_US));
    net.script.lock().unwrap().feed(1, b"late");
    net.wake();
    let out = finish(running);
    assert!(out.contains(&format!("{{ok,{}}}", text("late"))), "{out}");
    stop(net);
}

/// `ipd`'s refusals keep their names: the box's own addresses are `not_permitted` before any
/// stack (`eacces`), a connect the stack could not complete reads back closed (`econnrefused`),
/// a name has no resolver (`nxdomain`); and a socket that did not connect is aborted.
#[test]
fn refusals_are_named_and_a_failed_socket_is_given_back() {
    let _turn = turn();
    let net = fixture::net(&[PEER], &OWN, false);
    let out = finish(start("refused", Some(&net)));
    assert!(out.contains("{{error,eacces},{error,eacces},{error,econnrefused},{error,nxdomain}}"), "{out}");
    let seen = net.script.lock().unwrap().seen.clone();
    assert_eq!(
        seen,
        [
            NetSeen::Clone(1),
            NetSeen::Connect(1, [127, 0, 0, 1], 22),
            NetSeen::Abort(1),
            NetSeen::Clone(1),
            NetSeen::Connect(1, [10, 0, 2, 15], 22),
            NetSeen::Abort(1),
            NetSeen::Clone(1),
            NetSeen::Connect(1, [10, 0, 9, 200], 7),
            NetSeen::Abort(1),
        ]
    );
    stop(net);
}

/// A listener is `ctl listen(port, backlog)`; an accepted socket arrives through a read of the
/// listener's ctl, with its remote end from `remote`.
#[test]
fn a_listener_accepts_through_ctl() {
    let _turn = turn();
    let net = fixture::net(&[], &[], false);
    let running = start("listen", Some(&net));
    until(&net, "the listen", |s| s.seen.contains(&NetSeen::Listen(1, 8000, 2)));
    until(&net, "the accept's read", |s| !s.holds(1).is_empty());
    {
        let mut script = net.script.lock().unwrap();
        let m = script.accept(1, ([10, 0, 2, 2], 40000));
        script.feed(m, b"knock");
    }
    net.wake();
    let out = finish(running);
    assert!(out.contains(&format!("{{{{{{10,0,2,2}},40000}},{}}}", text("knock"))), "{out}");
    let seen = net.script.lock().unwrap().seen.clone();
    assert_eq!(
        seen,
        [
            NetSeen::Clone(1),
            NetSeen::Listen(1, 8000, 2),
            NetSeen::Wrote(2, b"knock".to_vec()),
            NetSeen::Close(2),
            NetSeen::Close(1)
        ]
    );
    stop(net);
}

/// Without a `/net` the loopback stand-in still serves the VM's own listeners, any other address
/// is `enetunreach`, and a name is `nxdomain`.
#[test]
fn without_a_net_the_in_vm_loopback_still_serves() {
    let _turn = turn();
    let out = finish(start("standin", None));
    assert!(out.contains(&format!("{{{},{{error,enetunreach}},{{error,nxdomain}}}}", text("loop"))), "{out}");
}
