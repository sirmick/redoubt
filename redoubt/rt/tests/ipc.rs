//! The runtime's system-call paths against the fake kernel: IPC with lends, transfers and
//! handles, `mint`, the heap over `map_anon`, and exits.

mod common;

use std::alloc::{GlobalAlloc, Layout};
use std::num::NonZeroU64;

use common::fake;
use redoubt_rt::abi::{Error, FOREVER, PAGE_SIZE};
use redoubt_rt::handle::{self, Endpoint};
use redoubt_rt::heap::Heap;
use redoubt_rt::ipc::{Buffer, Event};

fn nz(v: u64) -> NonZeroU64 { NonZeroU64::new(v).unwrap() }

#[test]
fn call_lend_and_reply() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[3, 1]));
    let receive = f.endpoint(server);
    let badged = f.grant(server, receive, client, 42);

    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let Ok(Event::Call(mut request)) = ep.receive(FOREVER, 0) else { return 1 };
        // What the kernel attaches: badge, account, labels.
        assert_eq!((request.caller.badge, request.caller.account), (42, 1001));
        assert_eq!(request.caller.labels.as_slice(), &[3, 1]);
        assert_eq!(request.words, [7, 8, 9, 10]);
        // The handle it brought is ours now; the lend is readable and writable.
        let brought = request.handles.as_slice()[0];
        let lend = request.lend();
        assert_eq!(lend.len(), 2 * PAGE_SIZE);
        assert_eq!(&lend[..5], b"hello");
        lend[..5].copy_from_slice(b"HELLO");
        // A handle to the same endpoint, minted from the message, goes back in the reply.
        let minted = request.mint(nz(77), None).unwrap();
        request.reply(&[1, 2, 3, u64::from(u32::MAX)], &[minted.handle(), brought]).unwrap();
        0
    });

    let code = f.run(client, move || {
        let ep = Endpoint::from_handle(badged);
        let mut lend = Buffer::new(2).unwrap();
        lend[..5].copy_from_slice(b"hello");
        let extra = Endpoint::create().unwrap();
        let reply = ep.call(&[7, 8, 9, 10], &[extra.handle()], Some(&mut lend), FOREVER).unwrap();
        assert_eq!(reply.words, [1, 2, 3, u64::from(u32::MAX)]);
        assert_eq!(reply.handles.as_slice().len(), 2);
        assert_eq!(&lend[..5], b"HELLO");
        0
    });
    assert_eq!(code.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
}

#[test]
fn send_transfers_pages_for_good() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(5, &[]));
    let receive = f.endpoint(server);
    let badged = f.grant(server, receive, client, 1);
    let before = f.held(server).1;

    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let Ok(Event::Send(delivery)) = ep.receive(FOREVER, 4) else { return 1 };
        let transfer = delivery.transfer.expect("pages");
        assert_eq!(&transfer[..4], b"gift");
        assert_eq!(delivery.words, [9, 0, 0, 0]);
        let mapped = fake().held(server).1;
        drop(transfer);
        // Dropping the buffer unmaps it: the pages were ours.
        assert_eq!(fake().held(server).1, mapped - 3);
        0
    });
    let client_thread = f.run(client, move || {
        let mut gift = Buffer::new(3).unwrap();
        gift[..4].copy_from_slice(b"gift");
        let held = fake().held(client).1;
        Endpoint::from_handle(badged)
            .send(&[9, 0, 0, 0], &[], Some(gift), FOREVER)
            .map_err(|(e, _)| e)
            .unwrap();
        // Gone from the sender.
        assert_eq!(fake().held(client).1, held - 3);
        0
    });
    assert_eq!(client_thread.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
    assert_eq!(f.held(server).1, before);
}

#[test]
fn timeouts_dead_endpoints_and_refusals() {
    let f = fake();
    let pid = f.process(0, &[]);
    let receive = f.endpoint(pid);
    let badged = f.grant(pid, receive, pid, 9);
    f.as_process(pid, || {
        let ep = Endpoint::from_handle(receive);
        assert!(matches!(ep.receive(1000, 0), Err(Error::Timeout)));
        assert_eq!(handle::sleep(1000), Ok(()));
        let other = Endpoint::from_handle(badged);
        // Only the receive right receives, and only it mints.
        assert!(matches!(other.receive(0, 0), Err(Error::NotPermitted)));
        assert_eq!(other.mint(nz(1), None), Err(Error::NotPermitted));
        // A call nobody takes times out.
        assert_eq!(other.call(&[0; 4], &[], None, 1000), Err(Error::Timeout));
        let t0 = handle::time_now().unwrap();
        assert!(handle::time_now().unwrap() >= t0);
        let mut bytes = [0u8; 200];
        handle::random(&mut bytes).unwrap();
        assert!(bytes.iter().any(|b| *b != 0));
        // Closing a handle makes it unusable.
        other.close().unwrap();
        assert_eq!(Endpoint::from_handle(badged).call(&[0; 4], &[], None, 0), Err(Error::BadHandle));
    });
    f.destroy(pid, receive);
    f.as_process(pid, || {
        assert!(matches!(Endpoint::from_handle(receive).receive(FOREVER, 0), Err(Error::Dead)))
    });
}

#[test]
fn exit_codes_reach_the_parent() {
    let f = fake();
    let pid = f.process(0, &[]);
    assert_eq!(f.run(pid, || handle::process_exit(12345)).join().unwrap(), 12345);
    assert_eq!(f.run(pid, || redoubt_rt::exit::OK).join().unwrap(), 0);
}

#[test]
fn heap_over_map_anon() {
    let f = fake();
    let pid = f.process(0, &[]);
    f.as_process(pid, || {
        let heap = Heap::new();
        let mut live: Vec<(*mut u8, Layout, u8)> = Vec::new();
        let mut x = 0x9e37_79b9_u64;
        for round in 0..20_000u32 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            if !x.is_multiple_of(3) || live.is_empty() {
                let size = [1, 8, 16, 17, 100, 2048, 2049, 5000, 3 * PAGE_SIZE][(x % 9) as usize];
                let align = [1, 8, 64, 4096][((x >> 8) % 4) as usize];
                let layout = Layout::from_size_align(size, align).unwrap();
                // SAFETY: the layout has a non-zero size.
                let ptr = unsafe { heap.alloc(layout) };
                assert!(!ptr.is_null());
                assert_eq!(ptr as usize % align, 0, "{layout:?}");
                let tag = round as u8;
                // SAFETY: the heap returned `size` writable bytes at `ptr`.
                unsafe { ptr.write_bytes(tag, size) };
                live.push((ptr, layout, tag));
            } else {
                let (ptr, layout, tag) = live.swap_remove((x as usize >> 3) % live.len());
                // SAFETY: `ptr` is live, from this heap, with this layout; no block overlapped it,
                // so its bytes are still the tag written at allocation.
                unsafe {
                    assert!(std::slice::from_raw_parts(ptr, layout.size()).iter().all(|b| *b == tag));
                    heap.dealloc(ptr, layout);
                }
            }
        }
        // Large blocks go back to the kernel; small ones stay on their lists.
        for (ptr, layout, _) in live.drain(..) {
            // SAFETY: as above.
            unsafe { heap.dealloc(ptr, layout) };
        }
        let small_pages = f.held(pid).1;
        let big = Layout::from_size_align(5 * PAGE_SIZE, 8).unwrap();
        // SAFETY: non-zero size; freed with the same layout.
        unsafe {
            let p = heap.alloc(big);
            assert_eq!(f.held(pid).1, small_pages + 5);
            heap.dealloc(p, big);
        }
        assert_eq!(f.held(pid).1, small_pages);
        // Alignment above a page cannot be had from map_anon.
        // SAFETY: non-zero size.
        assert!(unsafe { heap.alloc(Layout::from_size_align(8, 2 * PAGE_SIZE).unwrap()) }.is_null());
    });
}
