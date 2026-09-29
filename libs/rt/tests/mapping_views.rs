//! Mapping views are relinquished across IPC and reconstructed only when ownership returns.
//! This scripted ABI test checks runtime ownership, not kernel isolation.
#[path = "common/outcomes.rs"]
mod seam;

use std::num::NonZeroUsize;

use redoubt_rt::HostKernel;
use redoubt_rt::abi::*;
use redoubt_rt::handle::{Endpoint, map_anon};
use redoubt_rt::ipc::{Buffer, Event};
use redoubt_rt::server::typed::{Outcome, finish};

#[test]
fn mapping_reborrows_and_failed_reply_recovery() {
    let kernel = seam::Kernel::install();
    let endpoint = Endpoint::from_handle(Handle::new(1).unwrap());
    let mut buffer = Buffer::new(1).unwrap();
    assert_eq!(buffer.npages(), 1);
    assert!(buffer.iter().all(|byte| *byte == 0));
    buffer[..6].copy_from_slice(b"secret");
    assert_eq!(&buffer[..6], b"secret");
    assert!(!format!("{buffer:?}").contains("secret"));
    let mut result = endpoint.call(&[0; WORDS], &[], Some(buffer), FOREVER);
    let mut returned = result.buffer.take().unwrap();
    returned[PAGE_SIZE - 1] = 7;
    assert_eq!(returned[PAGE_SIZE - 1], 7);
    drop(returned);
    assert_eq!(kernel.0.lock().unwrap().unmapped, 1);

    // Map raw pages for the synthetic server receive: no caller-side Buffer remains aliased.
    let addr = map_anon(PAGE_SIZE, MemFlags::READ | MemFlags::WRITE).unwrap();
    kernel.request([0; WORDS], Some(Pages { addr, npages: NonZeroUsize::new(1).unwrap() }));
    let Event::Call(mut request) = endpoint.receive(FOREVER, 0).unwrap() else { panic!("not a call") };
    request.lend()[0] = 91;
    assert_eq!(request.lend()[0], 91);
    // A kernel rejection leaves the lend mapped for the fallback: `finish` answers malformed
    // and returns the first error, and the call is closed.
    kernel.0.lock().unwrap().reply = Err(Error::InvalidArgument);
    let answer = Outcome { words: [0; WORDS], send: Handles::new(), close: Handles::new() };
    assert_eq!(finish(request, &answer), Err(Error::InvalidArgument));
    {
        let s = kernel.0.lock().unwrap();
        assert_eq!(s.replies.len(), 2);
        assert_eq!(s.replies[1].words, redoubt_rt::server::MALFORMED.map(|word| word as usize));
        assert!(s.replies[1].handles.as_slice().is_empty());
        assert_eq!((s.open_calls, s.lent_pages), (0, 0), "the call is closed and its lend returned");
    }
    // The seam does not perform the real kernel's lend unmap; release its backing page now, as
    // the kernel would (the runtime's own unmap is its owners' alone).
    kernel.syscall(&Call::Unmap { addr, len: PAGE_SIZE }).unwrap();
}
