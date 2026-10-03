//! A rejected reply never closes a carried handle twice, nor one the server kept: `finish` and
//! `Parked::abandoned` empty the request's list before they reply, so the refusal a failed
//! reply's drop sends closes nothing. Against the scripted seam, which records every close.
use std::num::NonZeroU64;

use redoubt_fake_kernel::scripted as seam;
use redoubt_rt::abi::*;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Event, Request};
use redoubt_rt::server::parked::Parked;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::server::{Admission, AdmitKey, Limits};

const CARRIED: Handle = Handle::new(5).unwrap();

/// The next call, carrying [`CARRIED`].
fn receive(kernel: &seam::Kernel) -> Request {
    // SAFETY: the call names no address.
    unsafe {
        kernel.script(Received::Message(Message {
            kind: MessageKind::Call { lend: None },
            msg_id: NonZeroU64::new(1).unwrap(),
            badge: 1,
            account: 1001,
            labels: Labels::new(),
            body: ReceivedBody {
                words: [0; WORDS],
                handles: ReceivedHandles::from_slice(&[Some(CARRIED)]).unwrap(),
            },
        }))
    };
    let Event::Call(request) = Endpoint::from_handle(Handle::new(1).unwrap()).receive(FOREVER, 0).unwrap()
    else {
        panic!("not a call")
    };
    request
}

#[test]
fn a_rejected_reply_closes_no_carried_handle_twice() {
    let kernel = seam::Kernel::install();

    // Abandoned: the carried handle is closed once, though the reply is rejected and the
    // request's drop refuses it again.
    let mut admission = Admission::new(Limits { buckets: 4, in_flight: 8, files: 0, state: 0 }).unwrap();
    let mut parked: Parked<()> = Parked::new(FOREVER);
    let request = receive(kernel);
    let (id, key) = (request.id(), AdmitKey::of(&request.caller));
    assert!(parked.park(&mut admission, request, (key, 1), (), 0).is_ok());
    kernel.0.lock().unwrap().reply = Err(Error::InvalidArgument);
    assert!(parked.abandoned(&mut admission, id, &[0; WORDS]).is_some());
    {
        let s = kernel.0.lock().unwrap();
        assert_eq!(s.closed, [CARRIED], "the refusal on drop closed the carried handle again");
        assert_eq!((s.replies.len(), s.open_calls), (2, 0));
    }

    // Finished, keeping the carried handle: both replies rejected, so the server exits; the
    // request's drop on the way must not close what the server kept.
    kernel.0.lock().unwrap().closed.clear();
    let request = receive(kernel);
    {
        let mut s = kernel.0.lock().unwrap();
        s.reply = Err(Error::InvalidArgument);
        s.fallback = Err(Error::InvalidArgument);
    }
    let kept = Outcome { words: [0; WORDS], send: Handles::new(), close: Handles::new() };
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| finish(request, &kept))).is_err());
    let s = kernel.0.lock().unwrap();
    assert!(s.exited);
    assert!(s.closed.is_empty(), "a handle the server kept was closed");
}
