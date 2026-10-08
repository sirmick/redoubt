//! `bootfsd`'s own rules, without a kernel: the argument list, the setup protocol and the
//! read-only file server behind the skeleton. The whole program against a fake kernel, and the
//! 9P conformance vectors, are in `tests/`.

use alloc::vec;

use redoubt_rt::abi::Labels;
use redoubt_rt::server::ninep::mode;

use super::*;

fn caller(badge: u64) -> Caller { Caller { badge, account: 1001, labels: Labels::from_slice(&[]).unwrap() } }

/// `init`'s connection: a badge the launcher minted, below `FIRST_MINTED_BADGE`.
fn founder() -> Caller { caller(1) }

/// A client's connection, as `new_connection` mints it.
fn client() -> Caller { caller(FIRST_MINTED_BADGE + 3) }

/// The list as `init` writes it: each entry's length, a colon and its name.
fn sized(entries: &[(&str, usize)]) -> Vec<String> {
    entries.iter().map(|(name, len)| alloc::format!("{len}:{name}")).collect()
}

fn listed(entries: &[(&str, usize)]) -> Result<BootFs, SetupError> {
    BootFs::new(sized(entries).iter().map(String::as_str))
}

fn filled(entries: &[(&str, &[u8])]) -> BootFs {
    let list: Vec<(&str, usize)> = entries.iter().map(|(name, data)| (*name, data.len())).collect();
    let mut fs = listed(&list).expect("the public list");
    for (name, data) in entries {
        fs.handle(&founder(), Message::Add(Add { name, offset: 0, data }), &[]).expect("add");
    }
    fs.handle(&founder(), Message::Seal(Seal {}), &[]).expect("seal");
    fs
}

#[test]
fn the_public_list_is_checked_before_anything_is_served() {
    assert_eq!(listed(&[("a", 1), ("b", 2)]).map(|fs| fs.len()), Ok(2));
    assert_eq!(listed(&[("a", 1), ("a", 1)]).err(), Some(SetupError::Duplicate));
    for bad in ["", ".", "..", "a/b", "a\0b"] {
        assert_eq!(listed(&[(bad, 1)]).err(), Some(SetupError::BadName), "{bad:?}");
    }
    let many: Vec<(&str, usize)> = (0..=MAX_ENTRIES).map(|_| ("e", 1)).collect();
    let many: Vec<String> = many.iter().enumerate().map(|(i, (_, l))| alloc::format!("{l}:e{i}")).collect();
    assert_eq!(
        BootFs::new(many.iter().map(String::as_str)).err(),
        Some(SetupError::TooMany),
        "a list longer than MAX_ENTRIES"
    );
}

/// Each entry is `LENGTH:NAME`: the length canonical, the name split off at the first colon and
/// checked as before, and the lengths together within the bound, all before anything is served.
#[test]
fn an_entry_is_a_canonical_length_a_colon_and_a_name() {
    let one = |arg: &str| BootFs::new([arg].into_iter());
    assert_eq!(one("0:empty").map(|fs| fs.reserved("empty")), Ok(Some(0)));
    assert_eq!(one("65537:a").map(|fs| fs.reserved("a")), Ok(Some(65537)));
    // A name may hold a colon: the split is at the first.
    assert_eq!(one("3:erofsd:system").map(|fs| fs.entry("erofsd:system").is_some()), Ok(true));
    for bad in ["keyd", ":keyd", "007:keyd", "00:keyd", "1a:keyd", "-1:keyd", "99999999999999999999999:keyd"]
    {
        assert_eq!(one(bad).err(), Some(SetupError::BadLength), "{bad:?}");
    }
    for bad in ["1:", "1:.", "1:..", "1:a/b", "1:a\0b"] {
        assert_eq!(one(bad).err(), Some(SetupError::BadName), "{bad:?}");
    }
    assert_eq!(listed(&[("a", MAX_BYTES), ("b", 1)]).err(), Some(SetupError::TooLarge));
    assert_eq!(listed(&[("a", usize::MAX), ("b", 1)]).err(), Some(SetupError::TooLarge), "no overflow");
    assert_eq!(listed(&[("a", MAX_BYTES)]).map(|fs| fs.len()), Ok(1), "exactly the bound is fine");
}

/// An entry's buffer is reserved for its declared length at the start and never grown: the
/// server's heap holds the entries, whatever chunks the launcher pushes and wherever an image
/// falls against a power of two (doubling from a 60 KiB first chunk once put a 3.76 MiB image in
/// a 7.5 MiB buffer, with the old one live beside it).
#[test]
fn an_entry_is_reserved_once_for_its_length() {
    let len = (1 << 16) + 1;
    for chunk in [32 * 1024usize, 60 * 1024] {
        let mut fs = listed(&[("image", len)]).unwrap();
        assert_eq!(fs.reserved("image"), Some(len));
        let bytes = vec![7u8; len];
        for (i, part) in bytes.chunks(chunk).enumerate() {
            let add = Message::Add(Add { name: "image", offset: (i * chunk) as u64, data: part });
            fs.handle(&founder(), add, &[]).expect("add");
        }
        fs.handle(&founder(), Message::Seal(Seal {}), &[]).expect("seal");
        assert_eq!(fs.reserved("image"), Some(len), "chunks of {chunk}: grown");
        assert_eq!(fs.entry("image").map(<[u8]>::len), Some(len));
    }
}

/// A chunk that would pass the declared length is refused, and the entry stays as it was; a seal
/// while an entry is short of its length is refused, and `/boot` stays empty.
#[test]
fn a_chunk_past_the_length_and_a_seal_before_it_are_refused() {
    let mut fs = listed(&[("keyd", 5)]).unwrap();
    let add = |fs: &mut BootFs, offset, data: &[u8]| {
        fs.handle(&founder(), Message::Add(Add { name: "keyd", offset, data }), &[]).err()
    };
    assert_eq!(add(&mut fs, 0, b"abc"), None);
    assert_eq!(add(&mut fs, 3, b"def"), Some(ErrorCode::Refused), "past the length");
    assert_eq!(fs.entry("keyd"), Some(&b"abc"[..]));
    assert_eq!(fs.handle(&founder(), Message::Seal(Seal {}), &[]).err(), Some(ErrorCode::Refused), "short");
    assert!(!fs.sealed());
    assert_eq!(fs.walk(&client(), &Node::Root, "keyd").err(), Some(NineError::NOT_FOUND));
    assert_eq!(add(&mut fs, 3, b"de"), None);
    assert!(fs.handle(&founder(), Message::Seal(Seal {}), &[]).is_ok());
    assert!(fs.walk(&client(), &Node::Root, "keyd").is_ok());
}

/// `add` names an entry the list already holds, at exactly the offset reached so far, so a
/// chunk cannot be lost, repeated or reordered; anything else is refused.
#[test]
fn add_only_appends_to_a_listed_name_in_order() {
    let mut fs = listed(&[("keyd", 7), ("beamlet", 2)]).unwrap();
    let add = |fs: &mut BootFs, name, offset, data: &[u8]| {
        fs.handle(&founder(), Message::Add(Add { name, offset, data }), &[]).err()
    };
    assert_eq!(add(&mut fs, "keyd", 0, b"ELF"), None);
    assert_eq!(add(&mut fs, "keyd", 3, b"more"), None);
    assert_eq!(fs.entry("keyd"), Some(&b"ELFmore"[..]));
    // A name the list never held: refused exactly as a bad offset is.
    assert_eq!(add(&mut fs, "manifest.json", 0, b"secrets"), Some(ErrorCode::Refused));
    assert_eq!(add(&mut fs, "keyd", 0, b"again"), Some(ErrorCode::Refused));
    assert_eq!(add(&mut fs, "keyd", 8, b"gap"), Some(ErrorCode::Refused));
    assert_eq!(add(&mut fs, "keyd", u64::MAX, b"far"), Some(ErrorCode::Refused));
    assert_eq!(fs.entry("keyd"), Some(&b"ELFmore"[..]));
    assert_eq!(fs.entry("beamlet"), Some(&b""[..]));
}

/// Only the founding connection may fill `/boot`, and only before `seal`.
#[test]
fn setup_is_refused_after_seal_and_from_every_minted_connection() {
    let mut fs = listed(&[("keyd", 1)]).unwrap();
    let add = Message::Add(Add { name: "keyd", offset: 0, data: b"x" });
    assert_eq!(fs.handle(&client(), add, &[]).err(), Some(ErrorCode::Refused));
    assert_eq!(fs.handle(&client(), Message::Seal(Seal {}), &[]).err(), Some(ErrorCode::Refused));
    assert_eq!(fs.entry("keyd"), Some(&b""[..]));
    assert!(fs.handle(&founder(), add, &[]).is_ok());
    assert!(fs.handle(&founder(), Message::Seal(Seal {}), &[]).is_ok());
    // Sealed for good: not even the founder can add or seal again.
    assert_eq!(fs.handle(&founder(), add, &[]).err(), Some(ErrorCode::Refused));
    assert_eq!(fs.handle(&founder(), Message::Seal(Seal {}), &[]).err(), Some(ErrorCode::Refused));
}

/// Until `seal`, `/boot` is empty: no walk finds anything and no directory read lists anything,
/// so a client that gets there early cannot see a half-written entry.
#[test]
fn nothing_is_visible_before_seal() {
    let mut fs = listed(&[("keyd", 1)]).unwrap();
    fs.handle(&founder(), Message::Add(Add { name: "keyd", offset: 0, data: b"E" }), &[]).unwrap();
    assert_eq!(fs.walk(&client(), &Node::Root, "keyd").err(), Some(NineError::NOT_FOUND));
    assert_eq!(fs.dir_entry(&client(), &Node::Root, 0), Ok(None));
    fs.handle(&founder(), Message::Seal(Seal {}), &[]).unwrap();
    assert!(fs.walk(&client(), &Node::Root, "keyd").is_ok());
    assert!(fs.dir_entry(&client(), &Node::Root, 0).unwrap().is_some());
}

/// A walk to a name the list never held is "does not exist" — the same answer as for a name the
/// bundle never held, so `/boot` reveals nothing about the rest of the bundle
/// (servers/bootfsd.md R46).
#[test]
fn a_walk_to_an_unpublished_name_is_the_same_as_to_one_that_never_existed() {
    let mut fs = filled(&[("keyd", b"ELF"), ("beamlet", b"VM")]);
    let who = client();
    for name in ["manifest.json", "manifest", "kernel", "never-existed", "KEYD", "keyd "] {
        assert_eq!(fs.walk(&who, &Node::Root, name).err(), Some(NineError::NOT_FOUND), "{name:?}");
    }
    // Only the two published names, and only from the root.
    assert!(fs.walk(&who, &Node::Root, "keyd").is_ok());
    assert_eq!(fs.walk(&who, &Node::Entry(0), "keyd").err(), Some(NineError::NOT_FOUND));
}

#[test]
fn entries_read_back_byte_for_byte_at_any_offset() {
    let mut fs = filled(&[("keyd", b"hello, world")]);
    let who = client();
    let node = Node::Entry(0);
    let mut out = [0u8; 5];
    assert_eq!(fs.read(&who, &node, 0, &mut out), Ok(Read::Done(5)));
    assert_eq!(&out, b"hello");
    assert_eq!(fs.read(&who, &node, 7, &mut out), Ok(Read::Done(5)));
    assert_eq!(&out, b"world");
    // Past the end reads nothing, whatever the offset.
    assert_eq!(fs.read(&who, &node, 12, &mut out), Ok(Read::Done(0)));
    assert_eq!(fs.read(&who, &node, u64::MAX, &mut out), Ok(Read::Done(0)));
    assert_eq!(fs.read(&who, &Node::Root, 0, &mut out).err(), Some(NineError::NOT_FOUND));
}

/// Nothing writes. The read-only promise does not rest on the label check, which an unlabelled
/// caller passes.
#[test]
fn every_way_of_writing_is_refused() {
    let mut fs = filled(&[("keyd", b"ELF")]);
    let who = client();
    let node = Node::Entry(0);
    for m in [mode::OWRITE, mode::ORDWR, mode::OREAD | mode::OTRUNC, mode::OWRITE | mode::OTRUNC] {
        assert_eq!(fs.open(&who, &node, m).err(), Some(NineError::PERMISSION), "{m:#x}");
    }
    assert!(fs.open(&who, &node, mode::OREAD).is_ok());
    assert!(fs.open(&who, &node, mode::OEXEC).is_ok());
    assert_eq!(fs.write(&who, &node, 0, b"x").err(), Some(NineError::PERMISSION));
    assert_eq!(fs.create(&who, &Node::Root, "n", 0o644, mode::OWRITE).err(), Some(NineError::NOT_SUPPORTED));
    assert_eq!(fs.remove(&who, &node).err(), Some(NineError::NOT_SUPPORTED));
    assert_eq!(fs.entry("keyd"), Some(&b"ELF"[..]));
}

#[test]
fn the_directory_lists_exactly_the_public_list_in_order() {
    let mut fs = filled(&[("keyd", b"a"), ("beamlet", b"bb"), ("iex.beam", b"ccc")]);
    let who = client();
    let mut names = vec![];
    let mut i = 0;
    while let Some((_, stat)) = fs.dir_entry(&who, &Node::Root, i).unwrap() {
        names.push((stat.name.clone(), stat.length, stat.mode));
        i += 1;
    }
    assert_eq!(
        names,
        vec![("keyd".into(), 1, 0o444), ("beamlet".into(), 2, 0o444), ("iex.beam".into(), 3, 0o444)]
    );
    let root = fs.stat(&who, &Node::Root).unwrap();
    assert_eq!((root.name.as_str(), root.mode), ("/", DMDIR | 0o555));
}

/// The total is bounded at the list, so a launcher cannot make this server eat the machine's
/// memory, and no add passes an entry's declared length.
#[test]
fn the_published_bytes_are_bounded() {
    assert_eq!(listed(&[("big", MAX_BYTES + 1)]).err(), Some(SetupError::TooLarge));
    let mut fs = listed(&[("big", 1 << 16)]).unwrap();
    let chunk = vec![0u8; 1 << 16];
    let add = |fs: &mut BootFs, offset, data: &[u8]| {
        fs.handle(&founder(), Message::Add(Add { name: "big", offset, data }), &[]).err()
    };
    assert_eq!(add(&mut fs, 0, &chunk), None);
    assert_eq!(add(&mut fs, 1 << 16, &chunk[..1]), Some(ErrorCode::Refused));
    assert_eq!(fs.entry("big").map(<[u8]>::len), Some(1 << 16));
}

/// Every bucket at its cap fits the budget the manifest gives this server (servers/serving.md R26):
/// the image's 5 buckets and one more, and no more than [`BUDGET`] pays for.
#[test]
fn the_limits_fit_the_budget() {
    assert!(limits(6).fits(&COST, BUDGET));
    assert!(!limits(7).fits(&COST, BUDGET));
}
