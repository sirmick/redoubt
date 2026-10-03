//! A second transport under the runtime: one that counts each call and forwards it to the fake
//! kernel. Installed in the fake's place, it sees every call an echo round trip makes, and
//! nothing above the seam changes. One test per binary: the transport is installed once.

use std::sync::Mutex;

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Call, Error, FOREVER, Handles, Return, WORDS};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::{Transport, install_transport};

/// Every call that passed, by name.
struct Counting(Mutex<Vec<&'static str>>);

// SAFETY: forwards to the fake.
unsafe impl Transport for Counting {
    fn call(&self, call: &Call) -> Result<Return, Error> {
        self.0.lock().unwrap().push(call.number().name());
        // This transport's install took, so the fake's own is ignored: every call comes through here.
        fake().call(call)
    }
}

#[test]
fn a_second_transport_sees_every_call() {
    static COUNTING: Counting = Counting(Mutex::new(Vec::new()));
    assert!(install_transport(&COUNTING), "another transport was installed first");

    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive = f.endpoint(server);
    let badged = f.grant(server, receive, client, 7);

    let server_thread = f.run(server, move || {
        let Ok(Event::Call(request)) = Endpoint::from_handle(receive).receive(FOREVER, 0) else { return 1 };
        let words = request.words;
        let answer = Outcome { words, send: Handles::new(), close: Handles::new() };
        finish(request, &answer).map_or(2, |_| 0)
    });
    let client_thread = f.run(client, move || {
        let sent: [u64; WORDS] = [1, 2, 3, 4];
        let reply = Endpoint::from_handle(badged).call(&sent, &[], None, FOREVER).into_result();
        match reply {
            Ok((reply, None)) if reply.words == sent => 0,
            _ => 1,
        }
    });
    assert_eq!(client_thread.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);

    // What the fake saw, the counting transport saw first: the same calls, as many of each.
    let mut seen = COUNTING.0.lock().unwrap().clone();
    let mut faked = [f.calls(server), f.calls(client)].concat();
    assert!(seen.contains(&"call") && seen.contains(&"receive"), "{seen:?}");
    seen.sort_unstable();
    faked.sort_unstable();
    assert_eq!(seen, faked);
}
