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
    assert_eq!(h.take(7), Ok(false));
    assert_eq!(line(&mut h, "one"), Line::Queued);
    assert_eq!(line(&mut h, "two"), Line::Queued);
    assert_eq!(h.queued(), b"one\r\ntwo\r\n");
    assert_eq!(h.release(7, false), Some(false));
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
    assert_eq!(h.take(7), Ok(false));
    assert_eq!(h.take(7), Ok(false), "its own again is no change");
    assert_eq!(h.take(9), Err(Busy));
    assert_eq!(h.release(9, false), None, "a hold it does not have is no change");
    assert_eq!(h.holder(), Some(7));
    assert_eq!(h.release(7, false), Some(false));
    assert_eq!(h.release(7, false), None, "given back twice: the second is no change");
    assert_eq!(h.take(9), Ok(false));
}

/// A holder gives the hold back with its line unfinished: the next kernel line ends that line
/// first, once; the holder's next take says so, and only that take clears it.
#[test]
fn a_free_line_ends_an_unfinished_line_once_and_the_next_take_says_so() {
    let mut h = Hold::<64>::new();
    h.take(7).unwrap();
    assert_eq!(h.release(7, true), Some(false), "nothing waited: nothing to end yet");
    assert_eq!(line(&mut h, "[!] Terminating process with PID 5"), Line::Ending);
    assert_eq!(line(&mut h, "[!] Terminating process with PID 6"), Line::Free, "ended once");
    assert!(h.queued().is_empty());
    assert_eq!(h.take(7), Ok(true));
    assert_eq!(h.take(7), Ok(false), "answered once");
    assert_eq!(h.release(7, false), Some(false));
    assert_eq!(h.take(7), Ok(false));
}

/// Lines that waited while the holder's line was left unfinished go out at the give-back, so
/// that give-back ends the line first (`\r\n`), and the holder's next take is told, as for a
/// free line. A give-back with the line finished prints them bare, as before.
#[test]
fn lines_that_waited_end_an_unfinished_line_at_the_give_back_and_the_next_take_is_told() {
    let mut h = Hold::<64>::new();
    h.take(7).unwrap();
    assert_eq!(line(&mut h, "kill"), Line::Queued);
    assert_eq!(h.release(7, true), Some(true), "unfinished, with a line waiting: end it first");
    assert_eq!(h.queued(), b"kill\r\n");
    h.clear();
    assert_eq!(line(&mut h, "again"), Line::Free, "the give-back ended it");
    assert_eq!(h.take(7), Ok(true));
    assert_eq!(line(&mut h, "held"), Line::Queued);
    assert_eq!(h.release(7, false), Some(false), "finished: nothing to end");
    h.clear();
    assert_eq!(h.take(7), Ok(false));
}

/// The answer is the releaser's: another process's take after the kernel ended the line is told
/// nothing and consumes nothing, and the releaser hears of it at its own next take. The console
/// has one line: another holder that finishes it leaves nothing for the kernel to end.
#[test]
fn only_the_holder_whose_line_was_ended_is_told() {
    let mut h = Hold::<64>::new();
    h.take(7).unwrap();
    assert_eq!(h.release(7, true), Some(false));
    assert_eq!(line(&mut h, "kill"), Line::Ending, "7's line is ended");
    assert_eq!(h.take(9), Ok(false), "another's take: not told");
    assert_eq!(h.release(9, true), Some(false), "9's own unfinished line");
    assert_eq!(h.take(7), Ok(true), "7 is told, whatever 9 did meanwhile");
    assert_eq!(h.release(7, false), Some(false), "7 finished the one line, 9's tail included");
    assert_eq!(line(&mut h, "kill"), Line::Free, "nothing left to end");
    assert_eq!(h.take(9), Ok(false));
    assert_eq!(h.release(9, true), Some(false));
    assert_eq!(line(&mut h, "kill"), Line::Ending, "9's line is ended in its turn");
    assert_eq!(h.take(7), Ok(false), "7 is not told of 9's");
    assert_eq!(h.release(7, false), Some(false));
    assert_eq!(h.take(9), Ok(true));
}

/// Nothing is ended while no line is unfinished: a release that says the line is finished, or a
/// take and a release after an unfinished one. A held line waits as before, whatever is open.
#[test]
fn nothing_is_ended_while_no_line_is_unfinished_and_a_held_line_still_waits() {
    let mut h = Hold::<64>::new();
    h.take(7).unwrap();
    assert_eq!(h.release(7, false), Some(false));
    assert_eq!(line(&mut h, "kill"), Line::Free);
    assert_eq!(h.take(7), Ok(false));
    // Unfinished, then finished by the holder itself before any kernel line.
    assert_eq!(h.release(7, true), Some(false));
    assert_eq!(h.take(7), Ok(false), "no kernel line came");
    assert_eq!(line(&mut h, "held"), Line::Queued, "held: it waits, whole");
    assert_eq!(h.release(7, false), Some(false));
    assert_eq!(h.queued(), b"held\r\n");
    h.clear();
    assert_eq!(line(&mut h, "kill"), Line::Free, "the holder finished its line");
    assert_eq!(h.take(7), Ok(false));
    // A holder that dies with its line unfinished: the kernel ends it as it dies, nothing after.
    assert!(h.died(7));
    assert_eq!(line(&mut h, "kill"), Line::Free);
}

/// A process that dies between writes with its line unfinished is not the holder, but its line
/// is open: its death ends the line, before the kill's own, and nothing is owed to its PID after,
/// neither an ending nor an answer a later process with the reused PID could be given.
#[test]
fn a_process_that_dies_with_its_line_unfinished_is_owed_nothing_after() {
    let mut h = Hold::<64>::new();
    h.take(7).unwrap();
    assert_eq!(h.release(7, true), Some(false));
    assert!(h.died(7), "not holding, its line open: the kernel ends it now");
    assert_eq!(line(&mut h, "[!] Terminating process with PID 7"), Line::Free, "ended already");
    assert_eq!(h.take(7), Ok(false), "the reused PID is told nothing");
    // Ended by a kernel line first, then dead before it could take: the answer dies with it.
    assert_eq!(h.release(7, true), Some(false));
    assert_eq!(line(&mut h, "kill"), Line::Ending);
    assert!(!h.died(7), "nothing left to end");
    assert_eq!(h.take(7), Ok(false));
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
    assert!(!h.died(0) && h.release(0, false).is_none(), "PID 0 is nobody");
}
