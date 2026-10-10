//! The relay's decisions alone (servers/consrelay.md): the bound and the drop count, the line
//! cut, writes that never wait while detached, the replay on attach, the notes, and which
//! channel each helper thread is given.

use redoubt_consrelay::{Give, KEEP, MAX_INPUT, NO_SIZE, NOTE_CAP, Relay, Take, Thread};
use redoubt_rt::server::ninep::{Read, Write};

fn relay() -> Relay { Relay::new(vec![42]).expect("the buffers") }

/// Everything the writer on `generation` is given, until it would wait.
fn drain(r: &mut Relay, generation: u64) -> (Vec<u8>, Take) {
    let mut got = Vec::new();
    let mut out = [0u8; 4096];
    loop {
        match r.take_output(generation, &mut out) {
            Take::Bytes(n) => got.extend_from_slice(&out[..n]),
            other => return (got, other),
        }
    }
}

/// Attaches a console at `handle`, and has the writer take it and open it.
fn attach(r: &mut Relay, handle: u32, note: &str) -> u64 {
    r.attach(handle, note);
    let channel = r.next(Thread::Writer).expect("the writer is given the channel");
    assert_eq!(channel.handle, handle);
    r.opened(channel.generation, true);
    channel.generation
}

#[test]
fn a_write_never_waits_while_detached_and_the_newest_bytes_are_kept() {
    let mut r = relay();
    for _ in 0..3 {
        assert_eq!(r.output(&[b'x'; KEEP / 2 + 1]), Write::Done(KEEP / 2 + 1));
    }
    assert_eq!(r.kept(), KEEP);
    assert_eq!(r.dropped(), (3 * (KEEP / 2 + 1) - KEEP) as u64);
    // One write longer than the bound keeps its own tail.
    let mut r = relay();
    let mut long = vec![b'a'; KEEP];
    long.extend_from_slice(b"tail");
    assert_eq!(r.output(&long), Write::Done(KEEP + 4));
    assert_eq!(r.kept(), KEEP);
    assert_eq!(r.dropped(), 4);
    let g = attach(&mut r, 7, "");
    let (got, _) = drain(&mut r, g);
    assert!(got.ends_with(b"tail"), "the newest bytes are the ones kept");
}

#[test]
fn an_attach_cuts_to_a_line_says_what_was_dropped_and_replays() {
    let mut r = relay();
    r.output(b"lost\n");
    r.output(&[b'y'; KEEP - 3]);
    r.output(b"\x1b[1mhalf\nwhole line\r\n");
    // KEEP + 23 bytes written, so 23 dropped; then the cut takes the line cut short at the top,
    // escape sequence and all, up to its end: everything but the last line.
    assert_eq!(r.dropped(), 23);
    let g = attach(&mut r, 7, "[context work: reattached]\r\n");
    let (got, take) = drain(&mut r, g);
    assert_eq!(take, Take::Wait);
    let expected = format!(
        "[context work: reattached]\r\n[{} bytes of output dropped while detached]\r\nwhole line\r\n",
        KEEP + 11
    );
    assert_eq!(String::from_utf8(got).unwrap(), expected);
    assert_eq!(r.dropped(), 0);
}

#[test]
fn nothing_dropped_means_no_cut_and_no_count() {
    let mut r = relay();
    r.output(b"partial line, no end");
    let g = attach(&mut r, 7, "");
    let (got, _) = drain(&mut r, g);
    assert_eq!(got, b"partial line, no end");
}

#[test]
fn an_attached_write_waits_for_room_and_goes_out_in_order() {
    let mut r = relay();
    let g = attach(&mut r, 7, "[note]\r\n");
    assert_eq!(r.output(&vec![b'z'; KEEP]), Write::Done(KEEP));
    assert_eq!(r.output(b"more"), Write::Wait, "attached, a full buffer waits");
    let mut out = [0u8; 8];
    assert_eq!(r.take_output(g, &mut out), Take::Bytes(8));
    assert_eq!(&out, b"[note]\r\n", "the note goes first");
    assert_eq!(r.take_output(g, &mut out), Take::Bytes(8));
    assert_eq!(r.output(b"more"), Write::Done(4));
}

#[test]
fn a_broken_channel_keeps_output_as_if_detached() {
    let mut r = relay();
    let g = attach(&mut r, 7, "");
    r.output(&vec![b'z'; KEEP]);
    r.broken(g);
    assert_eq!(r.output(b"more"), Write::Done(4));
    assert_eq!(r.dropped(), 4);
    assert_eq!(r.take_output(g, &mut [0u8; 8]), Take::Gone);
}

#[test]
fn a_channel_let_go_is_gone_to_its_threads_and_a_takeover_takes_its_input() {
    let mut r = relay();
    let g = attach(&mut r, 7, "");
    assert!(r.next(Thread::Reader).is_some());
    assert_eq!(r.give_input(g, b"ls\n"), Give::Taken(3));
    assert!(r.readable());
    let g2 = attach(&mut r, 8, "");
    assert!(!r.readable(), "what was typed on the old channel is not the new one's");
    assert_eq!(r.give_input(g, b"x"), Give::Gone);
    assert_eq!(r.take_output(g, &mut [0u8; 8]), Take::Gone);
    assert_eq!(r.give_input(g2, b"y"), Give::Taken(1));
    let mut out = [0u8; 4];
    assert_eq!(r.input(&mut out), Read::Done(1));
    assert_eq!(r.input(&mut out), Read::Wait, "a console has no end");
}

#[test]
fn typed_input_outlives_a_detach() {
    let mut r = relay();
    let g = attach(&mut r, 7, "");
    assert_eq!(r.give_input(g, b"exit\n"), Give::Taken(5));
    assert!(!r.detach(""));
    let mut out = [0u8; 8];
    assert_eq!(r.input(&mut out), Read::Done(5), "the last line typed before the terminal closed");
    assert_eq!(&out[..5], b"exit\n");
}

/// The reader read `exit` before the steward's detach reached the relay, and gives it after:
/// it is still the VM's (the race steward-session-ends met).
#[test]
fn input_read_before_a_detach_and_given_after_it_is_the_vm_s() {
    let mut r = relay();
    let g = attach(&mut r, 7, "");
    assert!(!r.detach(""));
    assert_eq!(r.give_input(g, b"exit\n"), Give::Taken(5));
    let mut out = [0u8; 8];
    assert_eq!(r.input(&mut out), Read::Done(5));
    assert_eq!(&out[..5], b"exit\n");
    let g2 = attach(&mut r, 8, "");
    assert_eq!(r.give_input(g, b"x"), Give::Gone, "a channel attached since takes its place");
    assert_eq!(r.give_input(g2, b"y"), Give::Taken(1));
}

/// A takeover's detach carries a note: the old channel's unread input and anything its reader
/// brings in later are not the VM's.
#[test]
fn a_takeover_s_detach_drops_the_old_channel_s_input() {
    let mut r = relay();
    let g = attach(&mut r, 7, "");
    assert!(r.next(Thread::Reader).is_some());
    assert_eq!(r.give_input(g, b"ls\n"), Give::Taken(3));
    assert!(r.detach("[context default taken over from 192.0.2.1:22 at up 0h1m]\r\n"));
    assert!(!r.readable());
    assert_eq!(r.give_input(g, b"rm\n"), Give::Gone);
    assert!(!r.readable());
}

/// The attached channel's input ended (a channel without a pty, `ssh alice@box < file`): the VM
/// reads what it gave, then the end of the file. An end from a channel let go changes nothing,
/// and the next channel starts with no end.
#[test]
fn the_attached_channel_s_input_end_is_the_vm_s_end_of_file() {
    let mut r = relay();
    let g = attach(&mut r, 7, "");
    assert_eq!(r.give_input(g, b"1 + 1\n"), Give::Taken(6));
    r.input_ended(g);
    let mut out = [0u8; 8];
    assert_eq!(r.input(&mut out), Read::Done(6));
    assert_eq!(r.input(&mut out), Read::Done(0));
    assert!(r.readable());
    let g2 = attach(&mut r, 8, "");
    assert_eq!(r.input(&mut out), Read::Wait, "the next channel's input has not ended");
    r.input_ended(g);
    assert_eq!(r.input(&mut out), Read::Wait, "an end from the channel let go");
    assert!(!r.detach(""));
    r.input_ended(g2);
    assert_eq!(r.input(&mut out), Read::Wait, "an end after the detach: the channel's end, not its input's");
}

#[test]
fn input_is_bounded() {
    let mut r = relay();
    let g = attach(&mut r, 7, "");
    assert_eq!(r.give_input(g, &vec![b'k'; MAX_INPUT + 10]), Give::Taken(MAX_INPUT));
    assert_eq!(r.give_input(g, b"k"), Give::Taken(0));
}

#[test]
fn a_detach_waits_until_its_note_is_written() {
    let mut r = relay();
    let g = attach(&mut r, 7, "");
    r.output(b"unsent");
    assert!(r.detach("[context work taken over from 192.0.2.1:22 at up 0h01m]\r\n"));
    assert!(r.farewell_pending());
    let (got, take) = drain(&mut r, g);
    assert_eq!(got, b"[context work taken over from 192.0.2.1:22 at up 0h01m]\r\n");
    assert_eq!(take, Take::Gone);
    assert!(!r.farewell_pending());
    // What the old channel had not taken is kept for the next.
    let g2 = attach(&mut r, 8, "");
    assert_eq!(drain(&mut r, g2).0, b"unsent");
}

#[test]
fn a_detach_waits_for_nothing_it_cannot_write() {
    let mut r = relay();
    assert!(!r.detach("[note]\r\n"), "nothing attached");
    attach(&mut r, 7, "");
    assert!(!r.detach(""), "no note");
    r.attach(8, "");
    assert!(!r.detach("[note]\r\n"), "a channel not yet opened");
    let g3 = attach(&mut r, 9, "");
    r.broken(g3);
    assert!(!r.detach("[note]\r\n"), "a broken channel");
    // A writer that fails on the note ends the wait too.
    let g4 = attach(&mut r, 10, "");
    assert!(r.detach("[note]\r\n"));
    r.broken(g4);
    assert!(!r.farewell_pending());
}

#[test]
fn a_note_is_capped() {
    let mut r = relay();
    let long = "n".repeat(NOTE_CAP + 100);
    let g = attach(&mut r, 7, &long);
    assert_eq!(drain(&mut r, g).0.len(), NOTE_CAP);
}

#[test]
fn the_reader_is_given_a_channel_once_the_writer_opened_it() {
    let mut r = relay();
    assert_eq!(r.next(Thread::Writer), None);
    r.attach(7, "");
    assert_eq!(r.next(Thread::Reader), None, "not opened yet");
    let c = r.next(Thread::Writer).unwrap();
    assert_eq!(r.next(Thread::Writer), None, "the same channel is not given twice");
    r.opened(c.generation, true);
    assert_eq!(r.next(Thread::Reader).map(|c| c.handle), Some(7));
    // A console the writer could not open is never the reader's, and keeps output.
    r.attach(8, "");
    let c = r.next(Thread::Writer).unwrap();
    r.opened(c.generation, false);
    assert_eq!(r.next(Thread::Reader), None);
    assert_eq!(r.output(&vec![b'z'; KEEP + 1]), Write::Done(KEEP + 1));
}

#[test]
fn a_console_is_closed_once_no_thread_is_on_it() {
    let mut r = relay();
    r.attach(7, "");
    let c = r.next(Thread::Writer).unwrap();
    r.opened(c.generation, true);
    r.next(Thread::Reader).unwrap();
    r.attach(8, "");
    assert!(r.closable().is_empty(), "both threads are still on 7");
    r.next(Thread::Writer);
    assert!(r.closable().is_empty(), "the reader is");
    r.next(Thread::Reader);
    assert_eq!(r.closable(), vec![7]);
    // A console no thread was ever given is closed when it is let go; 8 the writer is still on.
    r.attach(9, "");
    r.attach(10, "");
    assert_eq!(r.closable(), vec![9]);
}

/// The console's size is the attached channel's: before any, `sshd`'s size for a channel without a
/// pty; a change counts once; each attach's first answer counts, so the VM redraws for its new
/// terminal; a let-go channel's answer is not taken.
#[test]
fn the_size_is_the_attached_channel_s_and_each_attach_is_a_change() {
    let mut r = relay();
    assert_eq!((r.size(), r.resized()), (NO_SIZE, 0));
    let g = attach(&mut r, 7, "");
    assert_eq!(r.next(Thread::Sizer).map(|c| c.generation), Some(g), "once the writer opened it");
    assert!(r.sized(g, (100, 40)));
    assert_eq!((r.size(), r.resized()), ((100, 40), 1));
    assert!(r.sized(g, (100, 40)));
    assert_eq!(r.resized(), 1, "the same size is no change");
    assert!(r.sized(g, (132, 43)));
    assert_eq!((r.size(), r.resized()), ((132, 43), 2));
    assert!(!r.detach(""));
    assert!(!r.sized(g, (90, 30)), "a channel let go is not the context's");
    assert_eq!(r.size(), (132, 43));
    let g2 = attach(&mut r, 8, "");
    assert!(r.sized(g2, (132, 43)));
    assert_eq!(r.resized(), 3, "a new channel's first answer counts, the same size or not");
    assert!(!r.sized(g, (1, 1)));
}
