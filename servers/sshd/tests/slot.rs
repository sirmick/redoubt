//! A slot comes back after every connection (servers/sshd.md, "The box's platform"), whichever
//! side ends it first. Before, a driver that had already taken the reader's `EOF` (the client
//! hung up first) waited for a second one that never came, and the slot never returned: four
//! ordinary sessions spent every slot.

use redoubt_sshd::slot::{DATA, EOF, Read, Reader};

const SLOTS: usize = 4;

/// One connection on `busy`'s first free slot, as the program's main thread and driver run it:
/// the reader's calls in `during` reach the driver while it runs the connection, and once it is
/// over the driver waits for the reader's last call only if it has not had it. Returns the slot,
/// or `None` if every slot was busy.
fn connection(busy: &mut [bool; SLOTS], during: &[u64]) -> Option<usize> {
    let s = busy.iter().position(|b| !b)?;
    busy[s] = true;
    let mut reader = Reader::default();
    for word in during {
        reader.took(*word);
    }
    if reader.waits() {
        // The socket is closed now; the reader's last call comes, and is taken.
        assert_eq!(reader.took(EOF), Read::End);
    }
    assert!(!reader.waits());
    busy[s] = false;
    Some(s)
}

#[test]
fn a_slot_returns_when_the_client_hangs_up_first() {
    let mut busy = [false; SLOTS];
    for n in 0..3 * SLOTS {
        assert_eq!(connection(&mut busy, &[DATA, DATA, EOF]), Some(0), "connection {n}");
    }
}

#[test]
fn a_slot_returns_when_the_server_ends_the_connection_first() {
    let mut busy = [false; SLOTS];
    for n in 0..3 * SLOTS {
        assert_eq!(connection(&mut busy, &[DATA]), Some(0), "connection {n}");
    }
}

#[test]
fn only_data_is_data() {
    let mut reader = Reader::default();
    assert_eq!(reader.took(DATA), Read::Data);
    assert!(reader.waits());
    assert_eq!(reader.took(EOF), Read::End);
    assert!(!reader.waits());
}
