#[path = "common/outcomes.rs"]
mod seam;

use redoubt_rt::abi::*;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Buffer;

#[test]
fn ownership_lifecycle_partial_reply_and_address_reuse() {
    let kernel = seam::Kernel::install();
    let ep = Endpoint::from_handle(Handle::new(1).unwrap());
    // Local encoding rejection never enters the kernel or consumes its buffer.
    let before = kernel.0.lock().unwrap().unmapped;
    let rejected =
        ep.call(&[0; WORDS], &[Handle::new(1).unwrap(); 5], Some(Buffer::new(1).unwrap()), FOREVER);
    assert_eq!(rejected.status, Err(Error::TooLarge));
    assert!(rejected.buffer.is_some());
    assert!(rejected.reply.is_none());
    drop(rejected);
    assert_eq!(kernel.0.lock().unwrap().unmapped, before + 1);
    // Queued cancellation, server death and late output failure all retain ownership.
    for status in [Err(Error::Timeout), Err(Error::Dead), Err(Error::InvalidArgument), Ok(())] {
        kernel.0.lock().unwrap().call =
            CallOutcome { status, lend: LendDisposition::Returned, reply_present: status.is_ok() };
        let mut buf = Buffer::new(1).unwrap();
        buf[0] = 19;
        let mut result = ep.call(&[0; WORDS], &[], Some(buf), FOREVER);
        assert_eq!(result.status, status);
        assert_eq!(result.reply.is_some(), status.is_ok());
        assert_eq!(result.buffer.as_ref().unwrap()[0], 19);
        let returned = result.buffer.take().unwrap();
        let before = kernel.0.lock().unwrap().unmapped;
        drop(result);
        assert_eq!(kernel.0.lock().unwrap().unmapped, before);
        drop(returned);
        assert_eq!(kernel.0.lock().unwrap().unmapped, before + 1);
    }
    // The replacement uses exactly the former address. Dropping the old outcome cannot unmap it.
    for error in [Error::Timeout, Error::Dead] {
        kernel.0.lock().unwrap().call =
            CallOutcome { status: Err(error), lend: LendDisposition::Consumed, reply_present: false };
        let buf = Buffer::new(1).unwrap();
        let addr = buf.as_ptr();
        let outcome = ep.call(&[0; WORDS], &[], Some(buf), FOREVER);
        assert!(outcome.buffer.is_none());
        let mut replacement = Buffer::new(1).unwrap();
        assert_eq!(replacement.as_ptr(), addr);
        replacement[0] = 91;
        let before = kernel.0.lock().unwrap().unmapped;
        drop(outcome);
        assert_eq!(kernel.0.lock().unwrap().unmapped, before);
        assert_eq!(replacement[0], 91);
        drop(replacement);
    }
    let kept = Handle::new(77).unwrap();
    {
        let mut s = kernel.0.lock().unwrap();
        s.call =
            CallOutcome { status: Err(Error::OutOfMemory), lend: LendDisposition::None, reply_present: true };
        s.body.handles = ReceivedHandles::from_slice(&[Some(kept), None]).unwrap();
    }
    let mut partial = ep.call(&[0; WORDS], &[], None, FOREVER);
    assert_eq!(partial.status, Err(Error::OutOfMemory));
    let reply = partial.reply.take().unwrap();
    assert_eq!(reply.words[0], 42);
    assert_eq!(reply.handles.as_slice(), &[Some(kept), None]);
    drop(partial);
    assert!(kernel.0.lock().unwrap().closed.is_empty(), "taken reply owns its handles");
    redoubt_rt::handle::close(kept).unwrap();
    assert_eq!(ep.call(&[0; WORDS], &[], None, FOREVER).into_result().unwrap_err(), Error::OutOfMemory);
    drop(ep.call(&[0; WORDS], &[], None, FOREVER));
    assert_eq!(
        kernel.0.lock().unwrap().closed,
        vec![kept; 3],
        "translate or drop closes partial handles exactly once"
    );
}
