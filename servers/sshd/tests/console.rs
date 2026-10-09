//! The box platform's channel console (`redoubt_sshd::console`): the file the session reads and
//! writes, and the session the core moves the channel's bytes through.

use std::cell::RefCell;
use std::rc::Rc;

use redoubt_rt::abi::Labels;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{FileServer, NineError, NineServer, Read, Write, mode};
use redoubt_sshd::console::{Chan, Cons, Console, Consoles, File, LIMITS, MAX_INPUT, MAX_OUTPUT};
use redoubt_sshd::{Session, Window};

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

/// Only a pty channel's end of input is a closed terminal, which detaches its context, and only
/// once the session has read all that was typed; its reads find no end of file until the channel
/// ends. Without a pty (`ssh alice@box < file`) it is the input's end, which the session reads as
/// the end of the file, and its channel stays.
#[test]
fn only_a_pty_channel_s_end_of_input_closes_the_terminal() {
    let (mut cons, mut session) = pair();
    session.start(None);
    assert_eq!(session.input(b"exit\n"), 5);
    session.input_ended();
    let mut out = [0u8; 8];
    assert_eq!(cons.read(&caller(), &File, 0, &mut out), Ok(Read::Done(5)));
    assert_eq!(cons.read(&caller(), &File, 0, &mut out), Ok(Read::Done(0)));
    assert!(!cons.chan.borrow().terminal_closed(), "a channel without a pty");
    let (mut cons, mut session) = pair();
    session.start(Some(Window { cols: 80, rows: 24 }));
    assert_eq!(session.input(b"exit\n"), 5);
    session.input_ended();
    assert!(!cons.chan.borrow().terminal_closed(), "the session has not read all that was typed");
    assert_eq!(cons.read(&caller(), &File, 0, &mut out), Ok(Read::Done(5)));
    assert!(cons.chan.borrow().terminal_closed());
    // No end of file before the channel's end, which follows the steward's detach.
    assert_eq!(cons.read(&caller(), &File, 0, &mut out), Ok(Read::Wait));
    cons.chan.borrow_mut().end(0);
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

/// `consol` on a channel: `size` is the pty's, 80 by 24 without one; a parked `resize` is due
/// when the window changes size, not when it is told the same size again, and when the session
/// ends, which is its last answer: a `consol` call after the end is refused. Each channel has its
/// own count, so another channel's change wakes nothing here.
#[test]
fn consol_size_is_the_pty_s_and_a_resize_is_due_when_it_changes() {
    let (cons, mut session) = pair();
    assert_eq!(cons.chan.borrow().size(), (80, 24));
    session.start(Some(Window { cols: 100, rows: 30 }));
    let chan = cons.chan.clone();
    assert_eq!((chan.borrow().size(), chan.borrow().resized), ((100, 30), 0));
    assert!(!chan.borrow().resize_due(0));
    session.window(Window { cols: 100, rows: 30 });
    assert!(!chan.borrow().resize_due(0), "the same size is no change");
    session.window(Window { cols: 132, rows: 43 });
    assert_eq!(chan.borrow().size(), (132, 43));
    assert!(chan.borrow().resize_due(0));
    assert!(!chan.borrow().resize_due(1));
    let (other, _) = pair();
    assert!(!other.chan.borrow().resize_due(0), "another channel's count is its own");
    assert_eq!(chan.borrow().consol_size(), Some((132, 43)));
    chan.borrow_mut().end(0);
    assert!(chan.borrow().resize_due(1), "an ended session answers every waiter");
    // That answer is the last: the next call, the VM's resize thread calling again, is refused.
    assert_eq!(chan.borrow().consol_size(), None);
}

/// Hands out handle numbers in place of the kernel's mints.
struct Mints(u32);

impl redoubt_rt::server::minted::Minter for Mints {
    fn mint(&mut self, _: std::num::NonZeroU64) -> Result<redoubt_rt::abi::Handle, redoubt_rt::abi::Error> {
        self.0 += 1;
        Ok(redoubt_rt::abi::Handle::new(self.0).unwrap())
    }

    fn random(&mut self) -> Result<u64, redoubt_rt::abi::Error> { Ok(0x5eed + u64::from(self.0)) }
}

/// A login mints two consoles, the steward's and its relay's, within the channel's limits; only
/// the steward's ends the channel, so the relay's `ended` and a session's do nothing. Once the
/// steward's has, a holder of the relay's reads the end and cannot write.
#[test]
fn a_login_s_two_consoles_and_only_the_steward_s_ends_the_channel() {
    let (cons, _session) = pair();
    let mut nine = NineServer::new(cons, LIMITS, 1).unwrap();
    let me = Caller { badge: 1, account: 0, labels: Labels::new() };
    let root = || (File, redoubt_sshd::console::qid());
    let (_, _, steward) = nine.mint_rooted(&me, root(), &mut Mints(10)).unwrap();
    let (_, _, relay) = nine.mint_rooted(&me, root(), &mut Mints(20)).unwrap();
    assert_ne!(steward, relay);
    let consoles = Consoles { steward, relay };
    assert!(consoles.may_end(steward));
    assert!(!consoles.may_end(relay), "the relay's console cannot end the channel");
    assert!(!consoles.may_end(caller().badge), "nor any other connection");
    // The steward's `ended` ends the channel: what the relay holds reads the end and cannot write.
    nine.fs.chan.borrow_mut().end(0);
    let relay_caller = Caller { badge: relay, account: 1001, labels: Labels::new() };
    let mut out = [0u8; 8];
    assert_eq!(nine.fs.read(&relay_caller, &File, 0, &mut out), Ok(Read::Done(0)));
    assert_eq!(nine.fs.write_or_wait(&relay_caller, &File, 0, b"x"), Err(NineError::NO_CONNECTION));
}
