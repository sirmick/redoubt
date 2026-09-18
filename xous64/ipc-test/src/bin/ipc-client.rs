#![no_std]
#![no_main]

use core::fmt::Write;

use ipc_test::{op, SERVER_ADDRESS};
use xous::{MemoryFlags, MemoryRange, MemorySize, Message, CID};

/// A page of memory that can be lent or moved to the server, and written to as text.
struct Page {
    range: MemoryRange,
    len: usize,
}

impl Page {
    fn new() -> Self {
        let range = xous::map_memory(None, None, 4096, MemoryFlags::R | MemoryFlags::W)
            .expect("couldn't allocate a page");
        Page { range, len: 0 }
    }

    fn bytes(&self) -> &[u8] { unsafe { core::slice::from_raw_parts(self.range.as_ptr(), self.len) } }

    fn valid(&self) -> Option<MemorySize> { MemorySize::new(self.len) }
}

impl Write for Page {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let page = unsafe { core::slice::from_raw_parts_mut(self.range.as_mut_ptr(), self.range.len()) };
        let dest = page.get_mut(self.len..self.len + s.len()).ok_or(core::fmt::Error)?;
        dest.copy_from_slice(s.as_bytes());
        self.len += s.len();
        Ok(())
    }
}

/// Print through the server by lending it a page of text.
fn log(cid: CID, page: &mut Page, args: core::fmt::Arguments) {
    page.len = 0;
    page.write_fmt(args).ok();
    xous::send_message(cid, Message::new_lend(op::PRINT, page.range, None, page.valid()))
        .expect("couldn't lend");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // The server may not have started yet; `connect` blocks until it exists.
    let cid = xous::connect(xous::SID::from_bytes(SERVER_ADDRESS).unwrap()).expect("couldn't connect");
    let mut page = Page::new();
    let mut failures = 0;
    let page_addr = page.range.as_ptr();
    log(cid, &mut page, format_args!("PID {} connected, page at {:p}", xous::current_pid().unwrap(), page_addr));

    // Scalar: fire and forget.
    xous::send_message(cid, Message::new_scalar(op::PRINT_SCALARS, 1, 2, 3, 4)).expect("scalar");

    // BlockingScalar: the reply comes back in registers. Use values that need 64 bits.
    let big = 1usize << 40;
    let sum = match xous::send_message(cid, Message::new_blocking_scalar(op::SUM, big, big, 3, 4)) {
        Ok(xous::Result::Scalar1(sum)) => sum,
        other => {
            log(cid, &mut page, format_args!("FAIL: unexpected SUM reply {:?}", other));
            0
        }
    };
    let ok = sum == 2 * big + 7;
    failures += !ok as usize;
    log(cid, &mut page, format_args!("{}: blocking scalar sum = {:#x}", if ok { "ok" } else { "FAIL" }, sum));

    // MutableBorrow: the server edits our page in place, and we get it back.
    let mut scratch = Page::new();
    scratch.write_str("lent mutably across address spaces").ok();
    xous::send_message(cid, Message::new_lend_mut(op::UPPERCASE, scratch.range, None, scratch.valid()))
        .expect("couldn't lend_mut");
    let ok = scratch.bytes() == b"LENT MUTABLY ACROSS ADDRESS SPACES";
    failures += !ok as usize;
    log(
        cid,
        &mut page,
        format_args!("{}: lend_mut returned {:?}", if ok { "ok" } else { "FAIL" }, core::str::from_utf8(scratch.bytes())),
    );

    // Lend the same page many times: every round trip unmaps it here, maps it in the
    // server, and reverses that on return. The mapping must survive.
    for round in 0..1000 {
        xous::send_message(cid, Message::new_lend_mut(op::UPPERCASE, scratch.range, None, scratch.valid()))
            .expect("couldn't lend_mut repeatedly");
        if scratch.bytes()[0] != b'L' {
            failures += 1;
            log(cid, &mut page, format_args!("FAIL: page contents lost on round {}", round));
            break;
        }
    }
    log(cid, &mut page, format_args!("ok: 1000 lend_mut round trips"));

    // Move: the page leaves our address space for good.
    let mut gift = Page::new();
    write!(gift, "this page now belongs to the server").ok();
    let gift_addr = gift.range.as_ptr() as usize;
    xous::send_message(cid, Message::Move(xous::MemoryMessage { id: op::PRINT_AND_KEEP, buf: gift.range, offset: None, valid: gift.valid() }))
        .expect("couldn't move");
    // The address must be unmapped here now, so mapping a fresh page there must succeed.
    let remap = xous::map_memory(None, xous::MemoryAddress::new(gift_addr), 4096, MemoryFlags::R | MemoryFlags::W);
    let ok = remap.is_ok();
    failures += !ok as usize;
    log(cid, &mut page, format_args!("{}: moved page's address is free again", if ok { "ok" } else { "FAIL" }));

    if failures == 0 {
        log(cid, &mut page, format_args!("IPC TEST PASSED"));
    } else {
        log(cid, &mut page, format_args!("IPC TEST FAILED: {} failure(s)", failures));
    }
    loop {
        xous::yield_slice();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        xous::yield_slice();
    }
}
