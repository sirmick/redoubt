//! The box platform's channel console (`redoubt_sshd::console`): the file the session reads and
//! writes, and the session the core moves the channel's bytes through.

use std::cell::RefCell;
use std::rc::Rc;

use redoubt_rt::abi::Labels;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{FileServer, NineError, NineServer, Read, Write, mode};
use redoubt_sshd::Session;
use redoubt_sshd::console::{Chan, Cons, Console, File, LIMITS, MAX_INPUT, MAX_OUTPUT};

fn caller() -> Caller { Caller { badge: 1 << 63, account: 1001, labels: Labels::new() } }

fn pair() -> (Cons, Console) {
    let chan = Rc::new(RefCell::new(Chan::default()));
    (Cons { chan: chan.clone(), labels: vec![] }, Console::new(chan, 7, false))
}

#[test]
fn a_channel_s_skeleton_is_sized_as_admission_allows() {
    let (cons, _) = pair();
    assert!(NineServer::new(cons, LIMITS, 1).is_ok());
}

#[test]
fn a_read_waits_for_input_and_takes_what_was_typed() {
    let (mut cons, mut session) = pair();
    let mut out = [0u8; 8];
    assert_eq!(cons.read(&caller(), &File, 0, &mut out), Ok(Read::Wait));
    assert!(!cons.chan.borrow().readable());
    assert_eq!(session.input(b"ls\n"), 3);
    assert!(cons.chan.borrow().readable());
    assert_eq!(cons.read(&caller(), &File, 0, &mut out), Ok(Read::Done(3)));
    assert_eq!(&out[..3], b"ls\n");
    // The interrupt is the session's own key, Ctrl+\, which no screen can take.
    session.interrupt();
    assert_eq!(cons.read(&caller(), &File, 0, &mut out), Ok(Read::Done(1)));
    assert_eq!(out[0], 0x1C);
}

#[test]
fn input_past_its_bound_stays_with_the_core() {
    let (_, mut session) = pair();
    let typed = vec![b'x'; MAX_INPUT + 10];
    assert_eq!(session.input(&typed), MAX_INPUT);
    assert_eq!(session.input(b"y"), 0);
}

#[test]
fn a_write_waits_for_room_and_the_channel_takes_it() {
    let (mut cons, mut session) = pair();
    let full = vec![b'o'; MAX_OUTPUT];
    assert_eq!(cons.write_or_wait(&caller(), &File, 0, &full), Ok(Write::Done(MAX_OUTPUT)));
    assert_eq!(cons.write_or_wait(&caller(), &File, 0, b"more"), Ok(Write::Wait));
    let mut buf = [0u8; 100];
    assert_eq!(session.output(&mut buf), 100);
    assert_eq!(cons.write_or_wait(&caller(), &File, 0, b"more"), Ok(Write::Done(4)));
}

#[test]
fn an_ended_session_reads_the_end_and_cannot_write() {
    let (mut cons, mut session) = pair();
    assert_eq!(cons.write_or_wait(&caller(), &File, 0, b"bye\n"), Ok(Write::Done(4)));
    assert_eq!(session.ended(), None);
    cons.chan.borrow_mut().end(0);
    assert_eq!(session.ended(), Some(0));
    let mut out = [0u8; 8];
    assert_eq!(cons.read(&caller(), &File, 0, &mut out), Ok(Read::Done(0)));
    assert_eq!(cons.write_or_wait(&caller(), &File, 0, b"x"), Err(NineError::NO_CONNECTION));
    // What it wrote before the end still reaches the channel.
    let mut buf = [0u8; 8];
    assert_eq!(session.output(&mut buf), 4);
    // The client's end of input is the end of the file too.
    let (mut cons, mut session) = pair();
    session.input_ended();
    assert_eq!(cons.read(&caller(), &File, 0, &mut out), Ok(Read::Done(0)));
}

#[test]
fn the_console_is_one_file_carrying_the_channel_s_labels() {
    let chan = Rc::new(RefCell::new(Chan::default()));
    let mut cons = Cons { chan, labels: vec![7] };
    assert_eq!(cons.labels(&File), &[7]);
    // Reached only through the connection minted for the session.
    assert_eq!(cons.attach(&caller(), "").err(), Some(NineError::PERMISSION));
    assert_eq!(cons.walk(&caller(), &File, "x").err(), Some(NineError::NOT_DIR));
    assert_eq!(cons.open(&caller(), &File, mode::OTRUNC | mode::OWRITE).err(), Some(NineError::BAD_MODE));
    assert!(cons.open(&caller(), &File, mode::ORDWR).is_ok());
}
