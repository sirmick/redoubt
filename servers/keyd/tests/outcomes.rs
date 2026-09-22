#[path = "../../../libs/rt/tests/common/outcomes.rs"]
mod seam;

use redoubt_keyd::keys::Keys;
use redoubt_keyd::server::{BUDGET, COST, KeyServer, LIMITS};
use redoubt_rt::abi::*;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::wire::proto::keyd::{Grant, Message};

#[test]
fn serving_grant_rolls_back_discard_missing_capability_and_error() {
    let kernel = seam::Kernel::install();
    let ep = Endpoint::from_handle(Handle::new(1).unwrap());
    for result in [
        Ok(ReplyOutcome { delivered: false, installed: 0 }),
        Ok(ReplyOutcome { delivered: true, installed: 0 }),
        Err(Error::InvalidArgument),
        Err(Error::BadHandle),
        Ok(ReplyOutcome { delivered: true, installed: 1 }),
    ] {
        let keys = Keys::from_args(
            ["host,ssh_host,1111111111111111111111111111111111111111111111111111111111111111"].into_iter(),
        )
        .unwrap();
        let mut server = KeyServer::new(keys, LIMITS, &COST, BUDGET, 0x1234_5678).unwrap();
        for _ in 0..20 {
            let first_reply = {
                let mut state = kernel.0.lock().unwrap();
                state.reply = result;
                state.replies.len()
            };
            kernel.request(Message::Grant(Grant {}).encode(&mut []).unwrap(), None);
            let Event::Call(request) = ep.receive(FOREVER, 0).unwrap() else { panic!("request") };
            assert_eq!(server.serve(request), result.map(|_| ()));
            let kept = result.is_ok_and(|outcome| outcome.accepted(1));
            assert_eq!(server.granted(), usize::from(kept));
            assert_eq!(server.admission().keys(), usize::from(kept));
            let s = kernel.0.lock().unwrap();
            assert_eq!(s.replies[first_reply].handles.as_slice().len(), 1, "actual grant minted");
            assert_eq!(s.closed.last(), Some(&Handle::new(77).unwrap()), "server copy closed");
            assert_eq!(s.open_calls, 0, "reply or fallback closed the call");
            assert_eq!(s.lent_pages, 0, "reply or fallback released server lend charges");
            if result.is_err() {
                assert_eq!(s.replies.len(), first_reply + 2);
                let fallback = s.replies.last().unwrap();
                assert_eq!(fallback.words, redoubt_rt::server::MALFORMED.map(|word| word as usize));
                assert!(fallback.handles.as_slice().is_empty());
            }
            if kept {
                break;
            }
        }
    }

    // Even failure of the handle-free fallback cannot let a serving loop continue with a
    // lost obligation: the process exits, and R4b releases its held calls.
    let keys = Keys::from_args(
        ["host,ssh_host,1111111111111111111111111111111111111111111111111111111111111111"].into_iter(),
    )
    .unwrap();
    let mut server = KeyServer::new(keys, LIMITS, &COST, BUDGET, 0x1234_5678).unwrap();
    {
        let mut s = kernel.0.lock().unwrap();
        s.reply = Err(Error::BadHandle);
        s.fallback = Err(Error::InvalidArgument);
    }
    kernel.request(Message::Grant(Grant {}).encode(&mut []).unwrap(), None);
    let Event::Call(request) = ep.receive(FOREVER, 0).unwrap() else { panic!("request") };
    let exited = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| server.serve(request)));
    assert_eq!(exited.unwrap_err().downcast_ref::<u32>(), Some(&redoubt_rt::start::exit::PANIC));
    let s = kernel.0.lock().unwrap();
    assert!(s.exited);
    assert_eq!((s.open_calls, s.lent_pages), (0, 0));
}
