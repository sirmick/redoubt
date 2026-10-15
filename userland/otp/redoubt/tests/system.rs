//! The system's calls on Redoubt (docs/userland/beamlet.md, "Natives"): the platform's `System`
//! against the fake kernel, each call made as the VM makes it, its events taken as the VM takes them
//! (idling until one comes). The labels are the kernel's stamp; a bind is the files' namespace; a
//! typed call goes out on a pool thread and comes back as an event; a served endpoint's requests
//! arrive with their caller's badge, account and labels, and one never answered is answered by the
//! serve thread at its deadline; a launch's end comes back as an event naming its job.

use std::sync::{Arc, Mutex};

use beamlet_redoubt::Redoubt;
use beamlet_redoubt::fixture::{self, Dirs, Volume};
use beamlet_redoubt::io::MAX_WAITERS;
use beamlet_redoubt::system::MAX_BINDINGS;
use beamlet_vm::platform::{
    BudgetSpec, Event, FileError, Files, Identity, Launch, Message, Object, OpenMode, Platform, Refused,
    System,
};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event as IpcEvent;
use redoubt_rt::server::typed::{Outcome, finish};

/// The asker the tests' calls are made as.
const ME: u64 = 7;

/// Runs `test` on a platform with `/dev/cons`, a blank volume at `/home/alice`, and what `extra`
/// hands it (by name), on an unlabelled session process (the console serves no labelled one).
fn with_session(
    extra: impl FnOnce(usize) -> Vec<(&'static str, Handle)> + Send + 'static,
    test: impl FnOnce(&mut Redoubt, usize) + Send + 'static,
) {
    let f = fake();
    let console = fixture::console(Box::new(std::io::empty()), Box::new(Screen::default()));
    let volume: Volume = fixture::volume(2048, &["buckets=4"]);
    let (pid, block) = fixture::session_built(&console, &[("/home/alice", &volume)], &[], &[], extra);
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        let mut platform = Redoubt::new(&startup, Box::new(Dirs(Vec::new()))).expect("a platform");
        test(&mut platform, pid);
        0
    });
    assert_eq!(session.join().unwrap(), 0);
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
    f.destroy(console.pid, console.endpoint);
    let _ = console.thread.join();
}

#[derive(Clone, Default)]
struct Screen(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Screen {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

/// The next event, idling as the VM does until one comes.
fn next(p: &mut Redoubt) -> (u64, Event) {
    loop {
        if let Some(event) = p.poll() {
            return event;
        }
        p.idle(None);
    }
}

/// A server process answering every call with words `[0, words[0] + 1, 0, 0]` and, for a lent
/// call, `"pong"` in its lend; it ends when its endpoint does. Returns its pid and endpoint.
fn echo() -> (usize, Handle) {
    let f = fake();
    let pid = f.process(0, &[]);
    let endpoint = f.endpoint(pid);
    f.run(pid, move || {
        let e = Endpoint::from_handle(endpoint);
        loop {
            match e.receive(FOREVER, 16) {
                Ok(IpcEvent::Call(mut request)) => {
                    let n = request.words[0] + 1;
                    let lent = request.lend();
                    let len = lent.len().min(4);
                    lent[..len].copy_from_slice(&b"pong"[..len]);
                    let send = redoubt_rt::abi::Handles::new();
                    let words = [0, if len > 0 { 4 } else { n }, 0, 0];
                    let _ = finish(request, &Outcome { words, send, close: send });
                }
                Ok(_) => {}
                Err(_) => return 0,
            }
        }
    });
    (pid, endpoint)
}

/// Makes the file call `call` as [`ME`] until it answers, idling between as the VM does.
fn ask<T>(
    p: &mut Redoubt,
    mut call: impl FnMut(&mut Redoubt) -> Result<T, FileError>,
) -> Result<T, FileError> {
    loop {
        p.asker(Some(ME));
        let r = call(p);
        p.asker(None);
        if r.as_ref().err() != Some(&FileError::Later) {
            return r;
        }
        while p.finished() != Some(ME) {
            p.idle(None);
        }
    }
}

fn message(words: [u64; 4], buffer: Option<&[u8]>) -> Message {
    Message { words, buffer: buffer.map(<[u8]>::to_vec), handles: Vec::new() }
}

#[test]
fn the_labels_are_the_kernels_stamp_on_the_vms_own_send() {
    let f = fake();
    for labels in [&[][..], &[5, 9]] {
        // The console of the session's own label set, which the session may write.
        let console =
            fixture::console_labelled(Box::new(std::io::empty()), Box::new(Screen::default()), labels);
        let (pid, block) = fixture::session_built(&console, &[], &[], labels, |_| Vec::new());
        let read = f.run(pid, move || {
            let startup = fixture::startup(&block);
            let mut platform = Redoubt::new(&startup, Box::new(Dirs(Vec::new()))).expect("a platform");
            platform.labels().iter().fold(0, |n, l| n * 100 + *l as u32)
        });
        assert_eq!(read.join().unwrap(), if labels.is_empty() { 0 } else { 509 });
        f.destroy(console.pid, console.endpoint);
        let _ = console.thread.join();
    }
}

#[test]
fn a_labelled_sessions_child_takes_its_labels_and_runs() {
    let f = fake();
    for labels in [&[][..], &[5, 9]] {
        let console =
            fixture::console_labelled(Box::new(std::io::empty()), Box::new(Screen::default()), labels);
        let (pid, block) =
            fixture::session_built(&console, &[], &[], labels, move |pid| vec![("budget", f.budget(pid))]);
        let session = f.run(pid, move || {
            let startup = fixture::startup(&block);
            let mut p = Redoubt::new(&startup, Box::new(Dirs(Vec::new()))).expect("a platform");
            let spec = |labels: Option<Vec<u64>>| BudgetSpec {
                pages: 64,
                processes: 1,
                weight: 1,
                labels,
                ..Default::default()
            };
            // Labels left out: the session's own, so the kernel's rule for a user-class caller holds.
            let child = p.budget_create(&spec(None)).expect("a child");
            // Never fewer than the session's, never more: the kernel refuses each by its name. An
            // empty set given is not left out: it goes to the kernel as it is.
            if !labels.is_empty() {
                for fewer in [vec![9], vec![]] {
                    assert_eq!(p.budget_create(&spec(Some(fewer))).err(), Some(Refused("label_denied")));
                }
            }
            let more = Some([labels, &[11]].concat());
            assert_eq!(p.budget_create(&spec(more)).err(), Some(Refused("class_denied")));
            let launch = Launch {
                image: b"\x7fELF".to_vec(),
                budget: child.clone(),
                namespace: vec![],
                handles: vec![],
                args: vec![],
                stack_pages: None,
                heap_pages: None,
                serve: None,
            };
            p.launch(ME, 1, launch).unwrap();
            p.budget_destroy(&child).unwrap();
            let (asker, event) = next(&mut p);
            let Event::Exit { job, cause, .. } = event else { panic!("not an exit") };
            assert_eq!((asker, job, cause), (ME, 1, "killed"));
            0
        });
        assert_eq!(session.join().unwrap(), 0);
        f.destroy(console.pid, console.endpoint);
        let _ = console.thread.join();
    }
}

#[test]
fn a_lookup_gives_the_longest_prefix_and_the_rest_and_refuses_by_name() {
    with_session(
        |_| Vec::new(),
        |p, _| {
            let (_, rest) = p.lookup("/home/alice/notes/today.txt").unwrap();
            assert_eq!(rest, "notes/today.txt");
            assert_eq!(p.lookup("/home/bob").err(), Some(Refused("not_found")));
            assert_eq!(p.lookup("/home/alice/../bob").err(), Some(Refused("bad_name")));
            assert_eq!(p.lookup("keyd").err(), Some(Refused("not_found")));
        },
    );
}

#[test]
fn a_bind_is_the_files_namespace_and_one_connection() {
    with_session(
        |_| Vec::new(),
        |p, _| {
            let (home, _) = p.lookup("/home/alice").unwrap();
            p.bind("/h", &home).unwrap();
            // Made through one prefix, seen through the other: the same connection.
            ask(p, |p| p.make_dir("/h/notes")).unwrap();
            assert_eq!(ask(p, |p| p.list_dir("/home/alice")).unwrap(), [b"notes".to_vec()]);
            let table: Vec<String> = p.table().into_iter().map(|e| e.path).collect();
            assert_eq!(table, ["/dev/cons", "/home/alice", "/h"]);
            assert_eq!(p.bind("/x/../y", &home).err(), Some(Refused("bad_name")));
        },
    );
}

/// The namespace holds at most `MAX_BINDINGS` entries, its first ones included: a new prefix past
/// them is `system_limit`, and a prefix already bound is still replaced.
#[test]
fn binds_past_the_cap_are_refused_and_a_bound_prefix_is_still_replaced() {
    with_session(
        |_| Vec::new(),
        |p, _| {
            let (home, _) = p.lookup("/home/alice").unwrap();
            let paths = |p: &mut Redoubt| p.table().into_iter().filter(|e| e.path.starts_with('/')).count();
            let first = paths(p);
            for i in first..MAX_BINDINGS {
                p.bind(&format!("/b{i}"), &home).unwrap();
            }
            assert_eq!(p.bind("/one-more", &home).err(), Some(Refused("system_limit")));
            p.bind("/b63", &home).unwrap();
            p.bind("/home/alice", &home).unwrap();
            assert_eq!(paths(p), MAX_BINDINGS);
        },
    );
}

/// A server bound at one prefix after another, each ended before the next is bound, as a session's
/// `piped` is at every pipeline (docs/servers/piped.md): each ended server's waiter hands over its
/// end and gives its place up, with the handle it woke the VM through, so the ninth is served as
/// the first was, past the VM's `MAX_WAITERS` (a session's fifth pipeline's was once `eio`).
#[test]
fn servers_bound_and_ended_one_after_another_never_run_out_of_waiters() {
    const SERVERS: [&str; 9] = ["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8"];
    let f = fake();
    let volumes: Arc<Mutex<Vec<Volume>>> = Arc::default();
    let made = Arc::clone(&volumes);
    with_session(
        move |pid| {
            let mut made = made.lock().unwrap();
            SERVERS
                .iter()
                .map(|name| {
                    let volume = fixture::volume(2048, &["buckets=4"]);
                    let handle = f.grant(volume.littlefsd, volume.endpoint, pid, 0x60);
                    made.push(volume);
                    (*name, handle)
                })
                .collect()
        },
        move |p, pid| {
            let mut made = Vec::new();
            // Nothing is in flight before the first bind: what the VM holds now is what it holds
            // again once each server's waiter has returned, the one handle it added closed.
            let (waiters, handles) = (p.waiters(), f.held(pid).0);
            for (i, name) in SERVERS.iter().enumerate() {
                let (server, _) = p.lookup(name).unwrap();
                p.bind("/dev/pipe", &server).unwrap();
                made.push(ask(p, |p| p.make_dir("/dev/pipe/p")));
                assert_eq!(p.waiters(), waiters + 1, "server {i}: its waiter started");
                assert_eq!(volumes.lock().unwrap().remove(0).stop(), redoubt_rt::exit::OK);
                // As a session idles between pipelines: the ended server's waiter hands over its
                // end, which the VM takes, and the hub lets the waiter go.
                let until = p.monotonic_us() + 5_000_000;
                while p.waiters() != waiters && p.monotonic_us() < until {
                    let soon = p.monotonic_us() + 10_000;
                    p.idle(Some(soon));
                }
                assert_eq!(p.waiters(), waiters, "server {i}: its waiter handed its end over");
                // The waiter closes its handle as it returns, just after that: waited for, not
                // sampled.
                let closed = f.wait_held(pid, handles, std::time::Duration::from_secs(5));
                assert!(closed, "server {i}: the waiter's handle closed; the VM holds {}", f.held(pid).0);
            }
            assert_eq!(made, [Ok(()); SERVERS.len()]);
        },
    );
}

/// Servers for the places the VM has, each granted to the session as a named handle.
fn servers(
    f: &'static redoubt_fake_kernel::Fake,
    pid: usize,
    n: usize,
    made: &Mutex<Vec<Volume>>,
) -> Vec<(&'static str, Handle)> {
    const NAMES: [&str; 8] = ["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7"];
    let mut made = made.lock().unwrap();
    NAMES[..n]
        .iter()
        .map(|name| {
            let volume = fixture::volume(2048, &["buckets=4"]);
            let handle = f.grant(volume.littlefsd, volume.endpoint, pid, 0x60);
            made.push(volume);
            (*name, handle)
        })
        .collect()
}

/// Every place held by a server that has just ended, none of whose waiters has handed its end
/// over yet, and a fresh server needing one: the VM waits for a hand-over at the bound and serves
/// the fresh one (once, `eio`: a session's seventh pipeline under load, B61).
#[test]
fn a_fresh_connection_at_the_bound_waits_for_an_ended_waiters_hand_over() {
    let f = fake();
    let volumes: Arc<Mutex<Vec<Volume>>> = Arc::default();
    let made = Arc::clone(&volumes);
    with_session(
        move |pid| servers(f, pid, MAX_WAITERS, &made),
        move |p, _| {
            // The console holds one place; the rest go to servers, each used so its waiter runs.
            let places = MAX_WAITERS - p.waiters();
            for i in 0..places {
                let (server, _) = p.lookup(&format!("s{i}")).unwrap();
                p.bind(&format!("/s{i}"), &server).unwrap();
                assert_eq!(ask(p, |p| p.make_dir(&format!("/s{i}/p"))), Ok(()));
            }
            assert_eq!(p.waiters(), MAX_WAITERS);
            // A fresh server needs a place while every one is held; the servers holding them end
            // a moment later, as a session's ended pipeds do under load, their waiters' hand-overs
            // still to come when the request is made.
            let ending: Vec<Volume> = (0..places).map(|_| volumes.lock().unwrap().remove(0)).collect();
            let ender = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                ending.into_iter().map(Volume::stop).collect::<Vec<_>>()
            });
            let (fresh, _) = p.lookup(&format!("s{places}")).unwrap();
            p.bind("/fresh", &fresh).unwrap();
            let started = std::time::Instant::now();
            assert_eq!(ask(p, |p| p.make_dir("/fresh/p")), Ok(()));
            assert!(started.elapsed() >= std::time::Duration::from_millis(100), "waited for a hand-over");
            assert!(ender.join().unwrap().iter().all(|code| *code == redoubt_rt::exit::OK));
            assert_eq!(volumes.lock().unwrap().remove(0).stop(), redoubt_rt::exit::OK);
        },
    );
}

/// Every place held by a live server, and a fresh one needing a place: after the bound's wait the
/// request is refused by the VM's own name, `emfile`, not a device's `eio`, and the live servers
/// are still served.
#[test]
fn a_fresh_connection_past_the_bound_is_refused_emfile_and_the_live_ones_still_serve() {
    let f = fake();
    let volumes: Arc<Mutex<Vec<Volume>>> = Arc::default();
    let made = Arc::clone(&volumes);
    with_session(
        move |pid| servers(f, pid, MAX_WAITERS, &made),
        move |p, _| {
            let places = MAX_WAITERS - p.waiters();
            for i in 0..places {
                let (server, _) = p.lookup(&format!("s{i}")).unwrap();
                p.bind(&format!("/s{i}"), &server).unwrap();
                assert_eq!(ask(p, |p| p.make_dir(&format!("/s{i}/p"))), Ok(()));
            }
            let (fresh, _) = p.lookup(&format!("s{places}")).unwrap();
            p.bind("/fresh", &fresh).unwrap();
            let started = std::time::Instant::now();
            assert_eq!(ask(p, |p| p.make_dir("/fresh/p")), Err(FileError::Emfile));
            assert!(started.elapsed() >= std::time::Duration::from_millis(900), "waited the bound first");
            assert_eq!(ask(p, |p| p.make_dir("/s0/q")), Ok(()));
            for _ in 0..=places {
                assert_eq!(volumes.lock().unwrap().remove(0).stop(), redoubt_rt::exit::OK);
            }
        },
    );
}

/// How many pipelines a long session runs: each one `piped`, launched with `serve`, bound and ended.
const PIPELINES: usize = 1000;

/// Launches `launch` for `job` and plays its server with a file server on the endpoint the child
/// was given: the send right, and the server. The child is found by the send right the VM, `pid`,
/// holds to it: other tests of this binary launch into the same fake.
fn serve_launch(p: &mut Redoubt, pid: usize, job: u64, launch: Launch) -> (Object, Volume) {
    let conn = p.launch(ME, job, launch).unwrap().expect("a send right");
    let child = fake().served_child(pid).expect("the child");
    (conn, fixture::volume_served(&child, 256, &["buckets=4"]))
}

/// A server the VM launched with `serve` and bound at a prefix leaves nothing behind once its budget
/// is destroyed: the prefix is unbound with the budget, before any term is dropped, and the VM's
/// send right to it, the VM's own, is closed when its last term goes. A session's `piped` is one
/// such at every pipeline (docs/servers/piped.md), and after a thousand the namespace table and the
/// VM's handles are where they started (each pipeline once left one entry and one handle for the
/// VM's life).
#[test]
fn a_served_servers_binding_and_handle_go_with_its_budget() {
    let f = fake();
    with_session(
        move |pid| vec![("budget", f.budget(pid))],
        move |p, pid| {
            let spec = BudgetSpec { pages: 8, processes: 1, weight: 1, ..Default::default() };
            let table = |p: &mut Redoubt| p.table().into_iter().map(|e| e.path).collect::<Vec<_>>();
            let paths = table(p);
            // The VM's handles once the first pipeline is over: the first launch starts a job
            // thread, with its endpoint and the two rights it is woken and wakes through, which
            // the VM keeps and reuses for its life (`jobs.rs`).
            let mut handles = None;
            for i in 0..PIPELINES {
                let budget = p.budget_create(&spec).unwrap();
                let launch = Launch {
                    image: b"\x7fELF".to_vec(),
                    budget: budget.clone(),
                    namespace: vec![],
                    handles: vec![],
                    args: vec![],
                    stack_pages: None,
                    heap_pages: None,
                    serve: Some("serve".into()),
                };
                // The test plays the server, a file server on the endpoint the child was given.
                let (conn, volume) = serve_launch(p, pid, i as u64, launch);
                p.bind("/dev/pipe", &conn).unwrap();
                assert_eq!(ask(p, |p| p.make_dir("/dev/pipe/p")), Ok(()));
                assert!(table(p).contains(&"/dev/pipe".to_string()));
                // Nothing is in flight: the job runs, the VM holds its send right and the budget.
                let (waiters, before) = (p.waiters(), f.held(pid).0);
                // The server dies first here (on the machine the destroy kills it; the fake does
                // not run the child), so the destroy finds its waiter's hand-over coming.
                assert_eq!(volume.stop(), redoubt_rt::exit::OK);
                p.budget_destroy(&budget).unwrap();
                // Forgetting the connection waited for that hand-over: the hub has let the
                // waiter go before the destroy returned, with no idle in between.
                assert_eq!(p.waiters(), waiters - 1, "pipeline {i}: its waiter handed its end over");
                let (_, event) = next(p);
                assert!(matches!(event, Event::Exit { cause: "killed", .. }), "pipeline {i}: not its exit");
                // Unbound with the budget, while the terms are still held.
                assert_eq!(p.lookup("/dev/pipe/p").err(), Some(Refused("not_found")), "pipeline {i}");
                assert_eq!(table(p), paths, "pipeline {i}");
                // The terms gone, five handles leave what the VM held before the server was
                // stopped: the budget's with the destroy, the job's exit endpoint and process
                // handle with its exit, the send right with its last term, and the waiter's one,
                // which it closes as it returns, on its thread: waited for, not sampled.
                drop(conn);
                drop(budget);
                let closed = f.wait_held(pid, before - 5, std::time::Duration::from_secs(5));
                assert!(closed, "pipeline {i}: the waiter's handle closed; the VM holds {}", f.held(pid).0);
                match handles {
                    Some(h) => assert_eq!(before - 5, h, "the VM's handles after pipeline {i}"),
                    None => handles = Some(before - 5),
                }
            }
        },
    );
}

/// A forgotten connection's hub session is ended at once: a request on it afterwards (a stale
/// open file's clunk, at its term's collection) is refused rather than sent, so it reaches nobody,
/// though the VM's handle to the server, closed as its last term went, has its index again by
/// then as the next launch's send right.
#[test]
fn a_forgotten_connections_requests_reach_no_later_holder_of_its_index() {
    let f = fake();
    with_session(
        move |pid| vec![("budget", f.budget(pid))],
        move |p, pid| {
            let spec = BudgetSpec { pages: 8, processes: 1, weight: 1, ..Default::default() };
            let launch = |budget: &Object| Launch {
                image: b"\x7fELF".to_vec(),
                budget: budget.clone(),
                namespace: vec![],
                handles: vec![],
                args: vec![],
                stack_pages: None,
                heap_pages: None,
                serve: Some("serve".into()),
            };
            let first = p.budget_create(&spec).unwrap();
            let (conn, volume) = serve_launch(p, pid, 1, launch(&first));
            p.bind("/dev/pipe", &conn).unwrap();
            let how = OpenMode { write: true, create: true, ..OpenMode::default() };
            let file = ask(p, |p| p.open("/dev/pipe/f", how)).unwrap();
            let waiters = p.waiters();
            p.budget_destroy(&first).unwrap();
            let (_, event) = next(p);
            assert!(matches!(event, Event::Exit { job: 1, cause: "killed", .. }));
            // A second server, which the test plays: it answers two calls, each with a handle it
            // mints, and counts whatever reaches its endpoint after them.
            let second = p.budget_create(&spec).unwrap();
            let again = p.launch(ME, 2, launch(&second)).unwrap().expect("a send right");
            let probe = f.process(0, &[]);
            let receive = f.play(&f.served_child(pid).unwrap(), probe)[0];
            let stray = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let counted = Arc::clone(&stray);
            f.run(probe, move || {
                let e = Endpoint::from_handle(receive);
                let mut answered = 0;
                while let Ok(event) = e.receive(FOREVER, 16) {
                    if answered == 2 {
                        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        continue;
                    }
                    if let IpcEvent::Call(request) = event {
                        let minted = e.mint(core::num::NonZeroU64::new(77).unwrap(), None).unwrap().handle();
                        let mut send = redoubt_rt::abi::Handles::new();
                        let _ = send.push(minted);
                        let _ = finish(request, &Outcome { words: [0; 4], send, close: send });
                        answered += 1;
                    }
                }
                0
            });
            let returned = |p: &mut Redoubt, call: u64| {
                p.call(ME, call, &again, message([1, 0, 0, 0], None), 1_000_000).unwrap();
                let (_, Event::Reply { result, .. }) = next(p) else { panic!("not a reply") };
                result.unwrap().handles
            };
            // The first call starts the pool's thread, with the handles it keeps.
            let warm = returned(p, 3);
            // The forgotten connection's last term gone, its index is free, and the next handle
            // the VM gets, the one the second server's reply brings, takes it.
            let held = f.held(pid).0;
            drop(conn);
            assert_eq!(f.held(pid).0, held - 1);
            let reused = returned(p, 4);
            assert_eq!(f.held(pid).0, held);
            // The stale file's clunk, as its term's collection sends it: refused on the ended
            // session; sent, it would go out on the index and reach the second server.
            p.close(file);
            for _ in 0..10 {
                let soon = p.monotonic_us() + 10_000;
                p.idle(Some(soon));
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(
                stray.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "a request reached the new holder"
            );
            drop((warm, reused));
            // The server gone, the waiter returns, closing the handle it woke the VM through.
            let before = f.held(pid).0;
            assert_eq!(volume.stop(), redoubt_rt::exit::OK);
            // The VM takes the waiter's last hand-over by idling, and the hub lets it go; then the
            // waiter closes its handle, on its thread: waited for, not sampled. The bounds are
            // synchronization: the whole binary's tests run beside this one.
            let until = p.monotonic_us() + 5_000_000;
            while p.waiters() == waiters && p.monotonic_us() < until {
                let soon = p.monotonic_us() + 10_000;
                p.idle(Some(soon));
            }
            assert_ne!(p.waiters(), waiters, "the waiter handed its end over");
            let closed = f.wait_held(pid, before - 1, std::time::Duration::from_secs(5));
            assert!(closed, "the waiter's handle, closed as it returned; the VM holds {}", f.held(pid).0);
            drop(again);
            p.budget_destroy(&second).unwrap();
            let (_, event) = next(p);
            assert!(matches!(event, Event::Exit { job: 2, cause: "killed", .. }));
            f.destroy(probe, receive);
        },
    );
}

#[test]
fn a_bind_to_a_server_that_never_answers_is_refused_within_its_bound() {
    let f = fake();
    with_session(
        // An endpoint whose owner never receives on it.
        move |pid| {
            let silent = f.process(0, &[]);
            let endpoint = f.endpoint(silent);
            vec![("silent", f.copy(silent, endpoint, pid))]
        },
        |p, _| {
            let (silent, _) = p.lookup("silent").unwrap();
            let begun = std::time::Instant::now();
            assert_eq!(p.bind("/s", &silent).err(), Some(Refused("not_a_connection")));
            assert!(begun.elapsed() < std::time::Duration::from_secs(5), "{:?}", begun.elapsed());
        },
    );
}

#[test]
fn a_budget_is_no_connection_and_no_endpoint() {
    let f = fake();
    with_session(
        move |pid| vec![("budget", f.budget(pid))],
        |p, _| {
            let (budget, rest) = p.lookup("budget").unwrap();
            assert_eq!(rest, "");
            assert_eq!(p.bind("/b", &budget).err(), Some(Refused("not_a_connection")));
            assert_eq!(
                p.call(ME, 1, &budget, message([1, 0, 0, 0], None), 1000).err(),
                Some(Refused("wrong_object"))
            );
            let (home, _) = p.lookup("/home/alice").unwrap();
            assert_eq!(p.budget_usage(&home).err(), Some(Refused("wrong_object")));
            assert_eq!(p.budget_destroy(&home).err(), Some(Refused("wrong_object")));
        },
    );
}

#[test]
fn a_typed_call_goes_out_on_a_pool_thread_and_its_reply_is_an_event() {
    let f = fake();
    let (server, endpoint) = echo();
    with_session(
        move |pid| vec![("echo", f.grant(server, endpoint, pid, 9))],
        |p, _| {
            let (echo, _) = p.lookup("echo").unwrap();
            p.call(ME, 1, &echo, message([41, 0, 0, 0], None), 1_000_000).unwrap();
            p.call(ME, 2, &echo, message([7, 0, 0, 0], Some(b"ping")), 1_000_000).unwrap();
            // The VM is not waiting: the calls are out.
            let mut replies = Vec::new();
            while replies.len() < 2 {
                let (asker, event) = next(p);
                assert_eq!(asker, ME);
                let Event::Reply { call, result } = event else { panic!("not a reply") };
                let reply = result.unwrap();
                replies.push((call, reply.words, reply.buffer));
            }
            replies.sort_by_key(|r| r.0);
            assert_eq!(replies[0], (1, [0, 42, 0, 0], None));
            assert_eq!(replies[1], (2, [0, 4, 0, 0], Some(b"pong".to_vec())));
        },
    );
}

#[test]
fn requests_arrive_with_badge_account_and_labels_and_an_answer_reaches_the_caller() {
    let f = fake();
    let client = f.process(2002, &[5, 8]);
    let served = Arc::new(Mutex::new(None));
    let given = Arc::clone(&served);
    with_session(
        move |pid| {
            let receive = f.endpoint(pid);
            *given.lock().unwrap() = Some(f.grant(pid, receive, client, 33));
            vec![("service", receive)]
        },
        move |p, _| {
            let (service, _) = p.lookup("service").unwrap();
            p.serve(ME, &service).unwrap();
            // Served once: a second thread on it could not tell its answers from the first's.
            assert_eq!(p.serve(ME, &service).err(), Some(Refused("already_served")));
            let to = served.lock().unwrap().unwrap();
            let caller = f.run(client, move || {
                let (reply, _) =
                    Endpoint::from_handle(to).call(&[3, 0, 0, 0], &[], None, FOREVER).into_result().unwrap();
                reply.words[1] as u32
            });
            let (asker, event) = next(p);
            assert_eq!(asker, ME);
            let Event::Request { request, badge, account, labels, message } = event else {
                panic!("not a request")
            };
            assert_eq!((badge, account, labels, message.words), (33, 2002, vec![5, 8], [3, 0, 0, 0]));
            p.reply(&request.unwrap(), self::message([0, 99, 0, 0], None)).unwrap();
            assert_eq!(caller.join().unwrap(), 99);
        },
    );
}

#[test]
fn an_endpoint_served_stays_open_when_its_term_is_dropped() {
    let f = fake();
    // `giver` answers every call with a receive right of an endpoint `served` of its own: a handle
    // the VM gets at run time, which only the term the reply made holds.
    let giver = f.process(0, &[]);
    let asked = f.endpoint(giver);
    let served = f.endpoint(giver);
    f.run(giver, move || {
        let e = Endpoint::from_handle(asked);
        while let Ok(event) = e.receive(FOREVER, 0) {
            if let IpcEvent::Call(request) = event {
                let mut send = redoubt_rt::abi::Handles::new();
                let _ = send.push(served);
                let _ =
                    finish(request, &Outcome { words: [0; 4], send, close: redoubt_rt::abi::Handles::new() });
            }
        }
        0
    });
    let client = f.process(2002, &[]);
    let to_served = f.grant(giver, served, client, 33);
    with_session(
        move |pid| vec![("giver", f.grant(giver, asked, pid, 5))],
        move |p, _| {
            let (giver, _) = p.lookup("giver").unwrap();
            p.call(ME, 1, &giver, message([1, 0, 0, 0], None), 1_000_000).unwrap();
            let (_, Event::Reply { result, .. }) = next(p) else { panic!("not a reply") };
            let mut reply = result.unwrap();
            let endpoint = reply.handles.pop().expect("the receive right");
            p.serve(ME, &endpoint).unwrap();
            // The only term for it gone: the serve thread still holds the endpoint open.
            drop(endpoint);
            drop(reply);
            // Bounded on both sides, so an endpoint closed under its thread fails, not hangs.
            let caller = f.run(client, move || {
                match Endpoint::from_handle(to_served).call(&[7, 0, 0, 0], &[], None, 3_000_000).into_result()
                {
                    Ok((reply, _)) => reply.words[1] as u32,
                    Err(_) => 0,
                }
            });
            let deadline = p.monotonic_us() + 3_000_000;
            let event = loop {
                if let Some(event) = p.poll() {
                    break event;
                }
                assert!(p.monotonic_us() < deadline, "no request reached the VM");
                p.idle(Some(deadline));
            };
            let (_, Event::Request { request, badge, .. }) = event else { panic!("not a request") };
            assert_eq!(badge, 33);
            p.reply(&request.unwrap(), message([0, 8, 0, 0], None)).unwrap();
            assert_eq!(caller.join().unwrap(), 8);
        },
    );
}

/// A request that reaches the VM's endpoint before the VM idles is handed over by that idle, not by
/// the one after the next wake-up: idling takes what is already there and returns with it. (The
/// VM slept on such a request until something else woke it; with nothing else to come, for good.)
#[test]
fn a_request_already_waiting_ends_the_idle_that_takes_it() {
    let f = fake();
    let client = f.process(2002, &[5]);
    let served = Arc::new(Mutex::new(None));
    let given = Arc::clone(&served);
    with_session(
        move |pid| {
            let receive = f.endpoint(pid);
            *given.lock().unwrap() = Some(f.grant(pid, receive, client, 33));
            vec![("service", receive)]
        },
        move |p, _| {
            let (service, _) = p.lookup("service").unwrap();
            p.serve(ME, &service).unwrap();
            let to = served.lock().unwrap().unwrap();
            let caller = f.run(client, move || {
                let (reply, _) =
                    Endpoint::from_handle(to).call(&[3, 0, 0, 0], &[], None, FOREVER).into_result().unwrap();
                reply.words[1] as u32
            });
            // The request is parked and its wake-up queued at the VM's endpoint before the VM
            // idles: long enough for the serve thread on any host; too short, and the wake-up
            // arrives during the idle instead, which is the easy case.
            std::thread::sleep(std::time::Duration::from_millis(300));
            let deadline = p.monotonic_us() + 10_000_000;
            p.idle(Some(deadline));
            let left = deadline.saturating_sub(p.monotonic_us());
            let (_, Event::Request { request, .. }) = p.poll().expect("the idle handed the request over")
            else {
                panic!("not a request")
            };
            assert!(left > 5_000_000, "the idle slept on the request it had taken");
            p.reply(&request.unwrap(), message([0, 5, 0, 0], None)).unwrap();
            assert_eq!(caller.join().unwrap(), 5);
        },
    );
}

#[test]
fn a_request_never_answered_is_answered_by_the_serve_thread_at_its_deadline() {
    let f = fake();
    let client = f.process(2002, &[5]);
    let served = Arc::new(Mutex::new(None));
    let given = Arc::clone(&served);
    with_session(
        move |pid| {
            let receive = f.endpoint(pid);
            *given.lock().unwrap() = Some(f.grant(pid, receive, client, 33));
            vec![("service", receive)]
        },
        move |p, _| {
            let (service, _) = p.lookup("service").unwrap();
            p.serve(ME, &service).unwrap();
            let to = served.lock().unwrap().unwrap();
            let caller = f.run(client, move || {
                let (reply, _) =
                    Endpoint::from_handle(to).call(&[3, 0, 0, 0], &[], None, FOREVER).into_result().unwrap();
                reply.words[0] as u32
            });
            let (_, event) = next(p);
            // Dropped unanswered: the VM holds nothing, and the thread answers it `malformed`.
            drop(event);
            assert_eq!(caller.join().unwrap(), 1);
        },
    );
}

#[test]
fn a_launch_takes_what_it_is_given_and_its_end_is_an_event() {
    let f = fake();
    let budget = Arc::new(Mutex::new(None));
    let carved = Arc::clone(&budget);
    with_session(
        move |pid| {
            let b = f.budget(pid);
            *carved.lock().unwrap() = Some(b);
            vec![("budget", b)]
        },
        move |p, pid| {
            let (own, _) = p.lookup("budget").unwrap();
            let (home, _) = p.lookup("/home/alice").unwrap();
            let launch = |budget: &Object| Launch {
                image: b"\x7fELF".to_vec(),
                budget: budget.clone(),
                namespace: vec![("/data".into(), home.clone())],
                handles: vec![],
                args: vec!["-v".into()],
                stack_pages: None,
                heap_pages: None,
                serve: None,
            };
            // Not a budget: refused before any kernel call.
            assert_eq!(p.launch(ME, 1, launch(&home)).err(), Some(Refused("wrong_object")));
            assert!(p.launch(ME, 2, launch(&own)).unwrap().is_none());
            let held = budget.lock().unwrap().unwrap();
            assert_eq!(f.launched_in(pid, held).len(), 1);
            // Its budget destroyed, it is killed, and the notice comes back naming the job.
            p.budget_destroy(&own).unwrap();
            let (asker, event) = next(p);
            let Event::Exit { job, cause, code } = event else { panic!("not an exit") };
            assert_eq!((asker, job, cause, code), (ME, 2, "killed", 0));
        },
    );
}

/// What a session is told of itself is read from its arguments, and anything malformed tells
/// nothing: no principal, two of them, two contexts or two homes, a label that is not `NAME:ID`,
/// a home that is not a clean absolute path.
#[test]
fn a_sessions_identity_is_read_from_its_arguments() {
    let told = beamlet_redoubt::identity;
    let labels = vec![("alice-secrets".to_string(), 7)];
    let args = [
        "budget_pages=9",
        "principal=alice",
        "label=alice-secrets:7",
        "context=work",
        "home=/home/alice",
        "M",
    ];
    let (context, home) = (Some("work".into()), Some("/home/alice".into()));
    assert_eq!(told(&args), Some(Identity { principal: "alice".into(), labels, context, home }));
    assert_eq!(told(&["principal=bob"]), Some(Identity { principal: "bob".into(), ..Identity::default() }));
    for bad in [
        &["budget_pages=9"][..],
        &["principal=a", "principal=b"],
        &["principal=a", "label=x"],
        &["principal=a", "label=x:y"],
        &["principal=a", "context=x", "context=y"],
        &["principal=a", "home=/h", "home=/i"],
        &["principal=a", "home=h"],
        &["principal=a", "home=/h/../i"],
    ] {
        assert_eq!(told(bad), None, "{bad:?}");
    }
}

/// The platform tells a session what its startup block's arguments say, and a VM started with
/// none, as one under `init`, that it is no session.
#[test]
fn the_platform_tells_a_session_its_identity() {
    let f = fake();
    let console = fixture::console(Box::new(std::io::empty()), Box::new(std::io::sink()));
    let volume = fixture::volume(2048, &["buckets=4"]);
    let args = ["principal=alice", "label=alice-secrets:7"];
    let (pid, block) = fixture::session_with(&console, &[("littlefsd:data", &volume)], &args);
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        let mut p = Redoubt::new(&startup, Box::new(Dirs(Vec::new()))).expect("a platform");
        let labels = vec![("alice-secrets".to_string(), 7)];
        assert_eq!(p.identity(), Some(Identity { principal: "alice".into(), labels, ..Identity::default() }));
        0
    });
    assert_eq!(session.join().unwrap(), 0);
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
    f.destroy(console.pid, console.endpoint);
    let _ = console.thread.join();
    with_session(|_| Vec::new(), |p, _| assert_eq!(p.identity(), None));
}

#[test]
fn a_launch_that_serves_gives_the_child_the_receive_right_and_its_caller_a_send_right() {
    let f = fake();
    let budget = Arc::new(Mutex::new(None));
    let carved = Arc::clone(&budget);
    with_session(
        move |pid| {
            let b = f.budget(pid);
            *carved.lock().unwrap() = Some(b);
            vec![("budget", b)]
        },
        move |p, pid| {
            let (own, _) = p.lookup("budget").unwrap();
            let launch = Launch {
                image: b"\x7fELF".to_vec(),
                budget: own.clone(),
                namespace: vec![],
                handles: vec![],
                args: vec![],
                stack_pages: None,
                heap_pages: None,
                serve: Some("serve".into()),
            };
            let conn = p.launch(ME, 1, launch).unwrap().expect("a send right");
            let child = f.launched_in(pid, budget.lock().unwrap().unwrap()).remove(0);
            // The child's one handle is the receive right; the session keeps none of it, only the
            // send right it was given, with the server's own badge.
            assert_eq!(f.served(pid, &child, 0), Some(vec![1]));
            // Its last copy dropped, the send right is closed.
            drop(conn);
            assert_eq!(f.served(pid, &child, 0), Some(vec![]));
            p.budget_destroy(&own).unwrap();
            let (_, event) = next(p);
            assert!(matches!(event, Event::Exit { job: 1, cause: "killed", .. }));
        },
    );
}

/// Budgets carved to take the indices a destruction freed: as many as the VM's table could have free.
const REFILLS: usize = 16;

/// A budget the VM destroyed takes its handle with it, and the kernel gives the index to the next
/// handle: collecting the old term closes nothing, so the budgets carved after are still held (once,
/// a later `piped`'s budget lost its handle so, and its carve stayed out of the session for good).
#[test]
fn a_destroyed_budgets_index_is_not_closed_when_its_term_is_collected() {
    let f = fake();
    with_session(
        move |pid| vec![("budget", f.budget(pid))],
        |p, _| {
            let spec = BudgetSpec { pages: 8, ..Default::default() };
            let first = p.budget_create(&spec).unwrap();
            p.budget_destroy(&first).unwrap();
            let carved: Vec<Object> = (0..REFILLS).map(|_| p.budget_create(&spec).unwrap()).collect();
            drop(first);
            for budget in &carved {
                assert_eq!(p.budget_destroy(budget), Ok(()));
            }
        },
    );
}

/// What calls on a server the VM launched with `serve` return is stamped with the server's budget,
/// so it goes when the VM destroys that budget, index and all: collecting its terms closes nothing,
/// so the budgets carved after are still held (a pipeline's stage connections, which `piped`
/// mints).
#[test]
fn what_a_served_server_returned_goes_with_its_budget_and_is_not_closed_again() {
    let f = fake();
    with_session(
        move |pid| vec![("budget", f.budget(pid))],
        move |p, _| {
            let spec = BudgetSpec { pages: 8, processes: 1, weight: 1, ..Default::default() };
            let budget = p.budget_create(&spec).unwrap();
            let launch = Launch {
                image: b"\x7fELF".to_vec(),
                budget: budget.clone(),
                namespace: vec![],
                handles: vec![],
                args: vec![],
                stack_pages: None,
                heap_pages: None,
                serve: Some("serve".into()),
            };
            let conn = p.launch(ME, 1, launch).unwrap().expect("a send right");
            // The test plays the server, in its budget: every call is answered with a connection
            // it mints.
            let server = f.process(0, &[]);
            let receive = f.play(&f.last_launched().unwrap(), server)[0];
            f.run(server, move || {
                let e = Endpoint::from_handle(receive);
                while let Ok(event) = e.receive(FOREVER, 0) {
                    if let IpcEvent::Call(request) = event {
                        let minted = e.mint(core::num::NonZeroU64::new(77).unwrap(), None).unwrap().handle();
                        let mut send = redoubt_rt::abi::Handles::new();
                        let _ = send.push(minted);
                        let _ = finish(request, &Outcome { words: [0; 4], send, close: send });
                    }
                }
                0
            });
            p.call(ME, 2, &conn, message([1, 0, 0, 0], None), 1_000_000).unwrap();
            let (_, Event::Reply { result, .. }) = next(p) else { panic!("not a reply") };
            let returned = result.unwrap().handles;
            assert_eq!(returned.len(), 1);
            p.budget_destroy(&budget).unwrap();
            let (_, event) = next(p);
            assert!(matches!(event, Event::Exit { job: 1, cause: "killed", .. }));
            let carved: Vec<Object> = (0..REFILLS).map(|_| p.budget_create(&spec).unwrap()).collect();
            drop(returned);
            for budget in &carved {
                assert_eq!(p.budget_destroy(budget), Ok(()));
            }
        },
    );
}

#[test]
fn a_launch_past_its_jobs_is_refused_and_a_serves_endpoint_with_it() {
    let f = fake();
    with_session(
        move |pid| vec![("budget", f.budget(pid))],
        move |p, pid| {
            let (own, _) = p.lookup("budget").unwrap();
            let launch = |serve: Option<String>| Launch {
                image: b"\x7fELF".to_vec(),
                budget: own.clone(),
                namespace: vec![],
                handles: vec![],
                args: vec![],
                stack_pages: None,
                heap_pages: None,
                serve,
            };
            for job in 0..beamlet_redoubt::MAX_JOBS as u64 {
                p.launch(ME, job, launch(None)).unwrap();
            }
            let before = f.held(pid).0;
            assert_eq!(p.launch(ME, 99, launch(Some("serve".into()))).err(), Some(Refused("too_many")));
            // Refused before anything was made: no endpoint, no handle left behind.
            assert_eq!(f.held(pid).0, before);
            p.budget_destroy(&own).unwrap();
            for _ in 0..beamlet_redoubt::MAX_JOBS {
                let (_, event) = next(p);
                assert!(matches!(event, Event::Exit { cause: "killed", .. }));
            }
        },
    );
}
