//! Mapping views are relinquished across IPC and reconstructed only when ownership returns.
//! This scripted ABI test checks runtime ownership, not kernel isolation.
#[path = "common/outcomes.rs"]
mod seam;

use std::num::NonZeroUsize;

use redoubt_rt::abi::*;
use redoubt_rt::handle::{Endpoint, map_anon, unmap};
use redoubt_rt::ipc::{Buffer, Event};

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
    // A local encoding rejection retains the existing view without entering the kernel.
    let (error, mut request) = request.reply(&[0; WORDS], &[endpoint.handle(); 5]).unwrap_err();
    assert_eq!(error, Error::TooLarge);
    assert_eq!(request.lend()[0], 91);
    assert!(kernel.0.lock().unwrap().replies.is_empty());
    // A kernel rejection leaves the lend mapped, so the returned Request can adopt it again.
    kernel.0.lock().unwrap().reply = Err(Error::InvalidArgument);
    let (error, mut request) = request.reply(&[0; WORDS], &[]).unwrap_err();
    assert_eq!(error, Error::InvalidArgument);
    assert_eq!(request.lend()[0], 91);
    request.lend()[PAGE_SIZE - 1] = 29;
    assert_eq!(kernel.0.lock().unwrap().open_calls, 1);
    assert!(request.reply(&[0; WORDS], &[]).unwrap().delivered);
    assert_eq!(kernel.0.lock().unwrap().open_calls, 0);
    // The seam does not perform the real kernel's lend unmap; release its backing page now.
    unmap(addr, PAGE_SIZE).unwrap();
}
