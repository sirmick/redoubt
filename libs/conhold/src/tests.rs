//! The hold's rules (kernel/devices.md, "The console's one writer").

use core::fmt::Write;

use super::*;

/// A kernel line as `print!` writes it: the text, then CR LF.
fn line<const N: usize>(h: &mut Hold<N>, text: &str) -> Line { h.line(|w| write!(w, "{text}\r\n")) }

#[test]
fn a_line_with_nobody_holding_goes_out_at_once() {
    let mut h = Hold::<64>::new();
    assert_eq!(h.holder(), None);
    assert_eq!(line(&mut h, "[!] Terminating process with PID 5"), Line::Free);
    assert!(h.queued().is_empty());
}

#[test]
fn lines_wait_whole_while_held_and_go_out_in_order_at_release() {
    let mut h = Hold::<64>::new();
    assert_eq!(h.take(7), Ok(()));
    assert_eq!(line(&mut h, "one"), Line::Queued);
    assert_eq!(line(&mut h, "two"), Line::Queued);
    assert_eq!(h.queued(), b"one\r\ntwo\r\n");
    assert!(h.release(7));
    assert_eq!(h.holder(), None);
    // The caller prints them, then clears; the next line goes out at once.
    assert_eq!(h.queued(), b"one\r\ntwo\r\n");
    h.clear();
    assert_eq!(line(&mut h, "three"), Line::Free);
}

/// A holder that never gives the hold back delays the kernel's lines by a queue's worth at most:
/// a line that does not fit goes out with everything queued before it, in order.
#[test]
fn a_line_that_does_not_fit_goes_out_after_the_queue_and_leaves_nothing_behind() {
    let mut h = Hold::<12>::new();
    h.take(7).unwrap();
    assert_eq!(line(&mut h, "fits"), Line::Queued);
    assert_eq!(line(&mut h, "does not fit"), Line::Full);
    assert_eq!(h.queued(), b"fits\r\n", "no part of the long line is queued");
    assert_eq!(h.spilled(), 1);
    // The caller prints what is queued, then the line, and clears; the hold is still taken.
    h.clear();
    assert_eq!(h.holder(), Some(7));
    assert_eq!(line(&mut h, "ok"), Line::Queued);
    assert_eq!(h.queued(), b"ok\r\n");
}

#[test]
fn one_holder_at_a_time() {
    let mut h = Hold::<16>::new();
    assert_eq!(h.take(7), Ok(()));
    assert_eq!(h.take(7), Ok(()), "its own again is no change");
    assert_eq!(h.take(9), Err(Busy));
    assert!(!h.release(9), "a hold it does not have is no change");
    assert_eq!(h.holder(), Some(7));
    assert!(h.release(7));
    assert!(!h.release(7), "given back twice: the second is no change");
    assert_eq!(h.take(9), Ok(()));
}

#[test]
fn a_holder_that_dies_gives_the_hold_back_and_only_it_does() {
    let mut h = Hold::<32>::new();
    h.take(7).unwrap();
    assert_eq!(line(&mut h, "kill"), Line::Queued);
    assert!(!h.died(9), "another process's death changes nothing");
    assert_eq!(h.holder(), Some(7));
    assert!(h.died(7));
    assert_eq!(h.holder(), None);
    assert_eq!(h.queued(), b"kill\r\n");
    assert!(!h.died(0) && !h.release(0), "PID 0 is nobody");
}
