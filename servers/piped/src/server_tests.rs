//! The pipes' rules, on the file server alone: who may make, mint and open what, how a pipe fills
//! and drains, and how each end's going shows at the other.

use alloc::vec;

use redoubt_rt::abi::Labels;

use super::*;

fn caller(badge: u64) -> Caller { Caller { badge, account: 0, labels: Labels::from_slice(&[4]).unwrap() } }

/// The session's own connection.
fn session() -> Caller { caller(ROOT_BADGE) }

/// A connection the session minted for a stage.
fn stage(n: u64) -> Caller { caller(FIRST_MINTED_BADGE + n) }

/// Pipes with one pipe, `p`, made by the session; its two ends, as the session walks to them.
fn one_pipe() -> (Pipes, Node, Node) {
    let mut fs = Pipes::new();
    fs.attach(&session(), "").unwrap();
    let (dir, _) = fs.create(&session(), &Node::Root, "p", DMDIR | 0o700, mode::OREAD).unwrap();
    let (r, _) = fs.walk(&session(), &dir, READ_END).unwrap();
    let (w, _) = fs.walk(&session(), &dir, WRITE_END).unwrap();
    (fs, r, w)
}

fn read(fs: &mut Pipes, r: &Node, n: usize) -> Result<Read, NineError> {
    let mut out = vec![0; n];
    fs.read(&stage(1), r, 0, &mut out)
}

#[test]
fn only_the_sessions_own_badge_attaches_and_its_labels_are_the_pipes() {
    let mut fs = Pipes::new();
    assert_eq!(fs.attach(&caller(2), "").err(), Some(NineError::PERMISSION));
    assert_eq!(fs.attach(&stage(1), "").err(), Some(NineError::PERMISSION));
    assert_eq!(fs.attach(&session(), "x").err(), Some(NineError::NOT_FOUND));
    assert_eq!(fs.attach(&session(), "").unwrap().0, Node::Root);
    assert_eq!(fs.labels(&Node::Root), &[4]);
}

#[test]
fn only_the_session_makes_and_removes_pipes_and_only_in_the_root() {
    let (mut fs, r, _) = one_pipe();
    let dir = Node::Pipe(1);
    assert_eq!(fs.create(&stage(1), &Node::Root, "q", DMDIR, 0).err(), Some(NineError::PERMISSION));
    assert_eq!(fs.create(&session(), &Node::Root, "q", 0o600, 0).err(), Some(NineError::PERMISSION));
    assert_eq!(fs.create(&session(), &dir, "q", DMDIR, 0).err(), Some(NineError::PERMISSION));
    assert_eq!(fs.create(&session(), &Node::Root, "p", DMDIR, 0).err(), Some(NineError("file exists")));
    assert_eq!(fs.remove(&stage(1), &dir).err(), Some(NineError::PERMISSION));
    assert_eq!(fs.remove(&session(), &r).err(), Some(NineError::PERMISSION));
    fs.remove(&session(), &dir).unwrap();
    assert!(fs.is_empty());
    // A fid left on the removed pipe reaches nothing, and no later pipe of the same name.
    fs.create(&session(), &Node::Root, "p", DMDIR, 0).unwrap();
    assert_eq!(read(&mut fs, &r, 1).err(), Some(REMOVED));
}

#[test]
fn pipes_are_bounded() {
    let mut fs = Pipes::new();
    for i in 0..MAX_PIPES {
        fs.create(&session(), &Node::Root, &alloc::format!("{i}"), DMDIR, 0).unwrap();
    }
    assert_eq!(fs.create(&session(), &Node::Root, "more", DMDIR, 0).err(), Some(NineError::TOO_MANY));
    let long = "x".repeat(MAX_NAME + 1);
    fs.remove(&session(), &Node::Pipe(1)).unwrap();
    assert_eq!(fs.create(&session(), &Node::Root, &long, DMDIR, 0).err(), Some(NineError::BAD_NAME));
}

#[test]
fn a_connection_is_minted_only_at_one_end_and_holds_it() {
    let (mut fs, r, w) = one_pipe();
    assert_eq!(
        fs.minted(&session(), FIRST_MINTED_BADGE, 1, &Node::Root, 0).err(),
        Some(NineError::PERMISSION)
    );
    assert_eq!(
        fs.minted(&session(), FIRST_MINTED_BADGE, 1, &Node::Pipe(1), 0).err(),
        Some(NineError::PERMISSION)
    );
    fs.minted(&session(), FIRST_MINTED_BADGE + 1, 1, &w, 0).unwrap();
    fs.minted(&session(), FIRST_MINTED_BADGE + 2, 2, &r, 0).unwrap();
    assert_eq!(fs.holders("p"), Some((1, 1)));
    fs.disconnected(FIRST_MINTED_BADGE + 1);
    assert_eq!(fs.holders("p"), Some((0, 1)));
    assert!(fs.take_moved());
    // A badge never minted here is nobody's.
    fs.disconnected(FIRST_MINTED_BADGE + 9);
    assert_eq!(fs.holders("p"), Some((0, 1)));
}

#[test]
fn an_end_opens_only_its_own_way() {
    let (mut fs, r, w) = one_pipe();
    for (node, how) in [(&r, mode::OWRITE), (&r, mode::ORDWR), (&w, mode::OREAD), (&w, mode::ORDWR)] {
        assert_eq!(fs.open(&stage(1), node, how).err(), Some(NineError::PERMISSION), "{node:?} {how}");
    }
    assert_eq!(fs.open(&stage(1), &w, mode::OWRITE | mode::OTRUNC).err(), Some(NineError::PERMISSION));
    assert!(fs.open(&stage(1), &r, mode::OREAD).is_ok());
    assert!(fs.open(&stage(1), &w, mode::OWRITE).is_ok());
    // A stage's open holds nothing: its connection does.
    assert_eq!(fs.holders("p"), Some((0, 0)));
}

#[test]
fn a_write_takes_what_fits_and_waits_when_nothing_does() {
    let (mut fs, r, w) = one_pipe();
    let big = vec![7u8; PIPE_BYTES + 100];
    assert_eq!(fs.write_or_wait(&stage(1), &w, 0, &big), Ok(Write::Done(PIPE_BYTES)));
    assert_eq!(fs.write_or_wait(&stage(1), &w, 0, b"x"), Ok(Write::Wait));
    assert_eq!(read(&mut fs, &r, 10), Ok(Read::Done(10)));
    assert_eq!(fs.write_or_wait(&stage(1), &w, 0, &big), Ok(Write::Done(10)));
    assert_eq!(fs.buffered("p"), Some(PIPE_BYTES));
}

#[test]
fn a_read_waits_until_the_write_end_goes_then_reads_the_rest_and_the_end() {
    let (mut fs, r, w) = one_pipe();
    // No writer yet: the stage that will write may not have started.
    assert_eq!(read(&mut fs, &r, 4), Ok(Read::Wait));
    fs.minted(&session(), FIRST_MINTED_BADGE + 1, 1, &w, 0).unwrap();
    assert_eq!(read(&mut fs, &r, 4), Ok(Read::Wait));
    fs.write_or_wait(&stage(1), &w, 0, b"abcdef").unwrap();
    fs.disconnected(FIRST_MINTED_BADGE + 1);
    let mut out = [0u8; 4];
    assert_eq!(fs.read(&stage(2), &r, 0, &mut out), Ok(Read::Done(4)));
    assert_eq!(&out, b"abcd");
    assert_eq!(read(&mut fs, &r, 4), Ok(Read::Done(2)));
    assert_eq!(read(&mut fs, &r, 4), Ok(Read::Done(0)));
}

#[test]
fn a_write_is_refused_once_the_read_end_goes_and_what_was_buffered_goes_with_it() {
    let (mut fs, r, w) = one_pipe();
    // No reader yet: taken, for the reader that will come.
    assert_eq!(fs.write_or_wait(&stage(1), &w, 0, b"early"), Ok(Write::Done(5)));
    fs.minted(&session(), FIRST_MINTED_BADGE + 2, 2, &r, 0).unwrap();
    fs.disconnected(FIRST_MINTED_BADGE + 2);
    assert_eq!(fs.buffered("p"), Some(0));
    assert_eq!(fs.write_or_wait(&stage(1), &w, 0, b"late"), Err(STATE));
}

#[test]
fn the_sessions_open_fid_holds_an_end_until_its_own_clunk() {
    let (mut fs, _, _) = one_pipe();
    let dir = Node::Pipe(1);
    let (w1, _) = fs.walk(&session(), &dir, WRITE_END).unwrap();
    let (w2, _) = fs.walk(&session(), &dir, WRITE_END).unwrap();
    let (r, _) = fs.walk(&session(), &dir, READ_END).unwrap();
    fs.open(&session(), &w1, mode::OWRITE).unwrap();
    fs.open(&session(), &r, mode::OREAD).unwrap();
    assert_eq!(fs.holders("p"), Some((1, 1)));
    // Another fid on the same end, never opened, lets go of nothing.
    fs.clunk(&w2);
    assert_eq!(fs.holders("p"), Some((1, 1)));
    fs.write_or_wait(&session(), &w1, 0, b"z").unwrap();
    fs.clunk(&w1);
    assert_eq!(fs.holders("p"), Some((0, 1)));
    assert_eq!(fs.read(&session(), &r, 0, &mut [0; 4]), Ok(Read::Done(1)));
    assert_eq!(fs.read(&session(), &r, 0, &mut [0; 4]), Ok(Read::Done(0)));
}

#[test]
fn the_tree_is_the_root_the_pipes_and_their_two_ends() {
    let (mut fs, _, _) = one_pipe();
    let dir = Node::Pipe(1);
    assert_eq!(fs.walk(&session(), &Node::Root, "q").err(), Some(NineError::NOT_FOUND));
    assert_eq!(fs.walk(&session(), &dir, "x").err(), Some(NineError::NOT_FOUND));
    let (r, _) = fs.walk(&session(), &dir, READ_END).unwrap();
    assert_eq!(fs.walk(&session(), &r, "x").err(), Some(NineError::NOT_DIR));
    let names = |fs: &mut Pipes, dir: &Node| {
        (0..)
            .map_while(|i| fs.dir_entry(&session(), dir, i).unwrap())
            .map(|(_, stat)| stat.name)
            .collect::<alloc::vec::Vec<_>>()
    };
    assert_eq!(names(&mut fs, &Node::Root), ["p"]);
    assert_eq!(names(&mut fs, &dir), [READ_END, WRITE_END]);
}

#[test]
fn the_limits_a_session_passes_fit_the_budget_and_admission_takes_them() {
    assert!(limits(2).fits(&COST, BUDGET));
    assert!(!limits(3).fits(&COST, BUDGET));
    assert!(redoubt_rt::server::admit::Admission::new(limits(2)).is_ok());
}
