//! The system's calls on Redoubt (docs/userland/beamlet.md, "Natives"): the platform's `System`
//! against the fake kernel, each call made as the VM makes it, its events taken as the VM takes them
//! (idling until one comes). The labels are the kernel's stamp; a bind is the files' namespace; a
//! typed call goes out on a pool thread and comes back as an event; a served endpoint's requests
//! arrive with their caller's badge, account and labels, and one never answered is answered by the
//! serve thread at its deadline; a launch's end comes back as an event naming its job.

use std::sync::{Arc, Mutex};

use beamlet_redoubt::Redoubt;
use beamlet_redoubt::fixture::{self, Dirs, Volume};
use beamlet_vm::platform::{Event, FileError, Files, Launch, Message, Object, Platform, Refused, System};
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
            };
            // Not a budget: refused before any kernel call.
            assert_eq!(p.launch(ME, 1, launch(&home)).err(), Some(Refused("wrong_object")));
            p.launch(ME, 2, launch(&own)).unwrap();
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
