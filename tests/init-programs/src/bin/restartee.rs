//! `restartee`: a `servers` entry on the runtime's serving library, a 9P server with an empty
//! root that also answers [`redoubt_init_programs::restartee`]'s requests: it hands a client a
//! copy of its own console connection, answers with a reply the kernel rejects, and, once it
//! holds one client's `wait` and another's `fault`, faults while it serves the `fault`, so that
//! `init` restarts it and blames the second (servers/init.md, "Restarts and reboots"). It prints
//! nothing: its unlabelled client is the reporter, and `init`'s lines are the rest.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_init_programs::Empty;
use redoubt_init_programs::restartee::{CONSOLE, FAULT, MISREPLY, OK, PING, WAIT};
use redoubt_rt::abi::{Error, FOREVER, Handle, Handles, MAX_HANDLES};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Event, Request};
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::server::{Limits, MALFORMED};
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// Exit codes: what it lacked to start.
const NO_ENDPOINT: u32 = 2;
const NO_CONSOLE: u32 = 3;
const NO_RANDOM: u32 = 4;
const BAD_LIMITS: u32 = 5;

/// A few connections per bucket: its client attaches and mints two.
const LIMITS: Limits = Limits { buckets: 2, in_flight: 0, files: 4, state: 4, requests: 0, pages: 0 };

fn serve(startup: &Startup) -> u32 {
    let Some(endpoint) = startup.handle("restartee") else { return NO_ENDPOINT };
    let endpoint = Endpoint::from_handle(endpoint);
    let Some((_, console)) = startup.namespace().find(|(path, _)| *path == "/dev/cons") else {
        return NO_CONSOLE;
    };
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let Ok(mut server) = NineServer::new(Empty, LIMITS, random) else { return BAD_LIMITS };
    let mut held = Held::default();
    loop {
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                let _ = server.serve_with(request, |_, request| own(request, console, &mut held));
            }
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
            }
            Ok(_) => {}
            Err(_) => redoubt_init_programs::park(),
        }
    }
}

/// The calls it holds unanswered until it faults: one [`WAIT`] and one [`FAULT`], in whichever
/// order they come.
#[derive(Default)]
struct Held {
    wait: Option<Request>,
    fault: Option<Request>,
}

/// One of its own requests, answered through the serving library's one finish path, or held.
fn own(request: Request, console: Handle, held: &mut Held) -> Result<(), Error> {
    match request.words[0] {
        WAIT if held.wait.is_none() => held.wait = Some(request),
        FAULT if held.fault.is_none() => held.fault = Some(request),
        _ => return answer(request, console),
    }
    if let (Some(_), Some(fault)) = (&held.wait, &held.fault) {
        // The fault's caller is the one blamed: its call is made the thread's current one.
        let _ = fault.serve();
        panic!("restartee faults while it serves its client");
    }
    Ok(())
}

/// Answers one of its own requests.
fn answer(request: Request, console: Handle) -> Result<(), Error> {
    let mut close = Handles::new();
    for handle in request.handles.as_slice().iter().flatten() {
        let _ = close.push(*handle);
    }
    let mut send = Handles::new();
    let words = match request.words[0] {
        // Kept: the copy goes, and the server's own stays.
        CONSOLE => {
            let _ = send.push(console);
            OK
        }
        // The last slot, which it never fills: the kernel rejects the reply, and the library
        // answers malformed in its place.
        MISREPLY => {
            let _ = send.push(Handle::new(MAX_HANDLES as u32).expect("a slot is at least 1"));
            OK
        }
        PING => OK,
        _ => MALFORMED,
    };
    finish(request, &Outcome { words, send, close }).map(|_| ())
}
