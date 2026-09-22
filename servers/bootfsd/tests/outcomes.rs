#[path = "../../../libs/rt/tests/common/outcomes.rs"]
mod seam;

use redoubt_bootfsd::server::{BootFs, LIMITS};
use redoubt_rt::abi::*;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Event};
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::wire::proto::ninep_common::{Message, NewConnection};

#[test]
fn serving_connection_rolls_back_discard_missing_capability_and_error() {
    let kernel = seam::Kernel::install();
    let ep = Endpoint::from_handle(Handle::new(1).unwrap());
    for result in [
        Ok(ReplyOutcome { delivered: false, installed: 0 }),
        Ok(ReplyOutcome { delivered: true, installed: 0 }),
        Err(Error::InvalidArgument),
        Err(Error::BadHandle),
        Ok(ReplyOutcome { delivered: true, installed: 1 }),
    ] {
        let mut server =
            NineServer::new(BootFs::new(core::iter::empty()).unwrap(), LIMITS, 0x1234_5678).unwrap();
        for _ in 0..20 {
            let mut buf = Buffer::new(1).unwrap();
            let words =
                Message::NewConnection(NewConnection { root: "", quota: 0 }).encode(&mut buf).unwrap();
            let first_reply = {
                let mut state = kernel.0.lock().unwrap();
                state.reply = result;
                state.replies.len()
            };
            kernel.request(
                words,
                Some(Pages {
                    addr: buf.as_mut_ptr() as usize,
                    npages: core::num::NonZeroUsize::new(1).unwrap(),
                }),
            );
            let Event::Call(request) = ep.receive(FOREVER, 0).unwrap() else { panic!("request") };
            assert_eq!(server.serve(request), result.map(|_| ()));
            let kept = result.is_ok_and(|outcome| outcome.accepted(1));
            assert_eq!(server.connections(), usize::from(kept));
            assert_eq!(server.admission().keys(), usize::from(kept));
            let s = kernel.0.lock().unwrap();
            assert_eq!(s.replies[first_reply].handles.as_slice().len(), 1, "actual connection minted");
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

    let mut server = NineServer::new(BootFs::new(core::iter::empty()).unwrap(), LIMITS, 0x1234_5678).unwrap();
    let mut buf = Buffer::new(1).unwrap();
    let words = Message::NewConnection(NewConnection { root: "", quota: 0 }).encode(&mut buf).unwrap();
    {
        let mut s = kernel.0.lock().unwrap();
        s.reply = Err(Error::BadHandle);
        s.fallback = Err(Error::InvalidArgument);
    }
    kernel.request(
        words,
        Some(Pages { addr: buf.as_mut_ptr() as usize, npages: core::num::NonZeroUsize::new(1).unwrap() }),
    );
    let Event::Call(request) = ep.receive(FOREVER, 0).unwrap() else { panic!("request") };
    {
        let s = kernel.0.lock().unwrap();
        assert_eq!((s.open_calls, s.lent_pages), (1, 1));
    }
    let exited = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| server.serve(request)));
    assert_eq!(exited.unwrap_err().downcast_ref::<u32>(), Some(&redoubt_rt::start::exit::PANIC));
    let s = kernel.0.lock().unwrap();
    assert!(s.exited);
    assert_eq!((s.open_calls, s.lent_pages), (0, 0));
}
