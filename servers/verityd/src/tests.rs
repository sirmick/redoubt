//! Every property `verityd` claims, with a test that tries to break it
//! (docs/servers/verityd.md; TENETS.md 6). These drive [`server::answer_with`] against a fake
//! range, so every path runs with no system call.

extern crate std;

use alloc::vec;
use alloc::vec::Vec;

use redoubt_rt::abi::{Labels, ReceivedHandles};
use redoubt_rt::ipc::{Caller, Words};
use redoubt_rt::wire::proto::blkd::{ErrorCode, Flush, Info, Message, Read, Reply, Write};
use redoubt_verity::{BLOCK, Geometry, Hash, SECTORS_PER_BLOCK, build};

use super::*;
use crate::server::{BADGE, MAX_SECTORS, Said, answer_with};
use crate::volume::{Bad, TREE_CACHE};

const LEND: usize = 64 * 1024;

/// A range of bytes; `reads` counts the calls, and `fail` makes every call fail.
struct Fake {
    bytes: Vec<u8>,
    reads: u64,
    fail: bool,
}

impl Range for Fake {
    fn info(&mut self) -> Result<Size, Fault> {
        if self.fail {
            return Err(Fault);
        }
        Ok(Size { sectors: (self.bytes.len() / SECTOR as usize) as u64 })
    }

    fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.reads += 1;
        let at = usize::try_from(sector).map_err(|_| Fault)?.checked_mul(SECTOR as usize).ok_or(Fault)?;
        let from = self.bytes.get(at..at.checked_add(out.len()).ok_or(Fault)?).ok_or(Fault)?;
        if self.fail {
            return Err(Fault);
        }
        out.copy_from_slice(from);
        Ok(())
    }
}

/// A packed volume of `n` data blocks, block i filled with i's low byte and its number, its tree
/// after it, `slack` spare blocks after that, and its root.
fn packed(n: u64, slack: usize) -> (Geometry, Vec<u8>, Hash) {
    let g = Geometry::new(n).unwrap();
    let mut bytes = vec![0u8; ((g.total_sectors() / SECTORS_PER_BLOCK) as usize + slack) * BLOCK];
    for (i, block) in bytes[..n as usize * BLOCK].chunks_exact_mut(BLOCK).enumerate() {
        block.fill(i as u8);
        block[..8].copy_from_slice(&(i as u64).to_le_bytes());
    }
    let (data, rest) = bytes.split_at_mut(n as usize * BLOCK);
    let root = build(&g, data, &mut rest[..g.tree_blocks() as usize * BLOCK]).unwrap();
    (g, bytes, root)
}

fn server(g: Geometry, bytes: Vec<u8>, root: &Hash, labels: &[u64]) -> Verityd<Fake> {
    Verityd::new(Fake { bytes, reads: 0, fail: false }, g, root, labels.to_vec())
}

fn caller(badge: u64, labels: &[u64]) -> Caller {
    Caller { badge, account: 0, labels: Labels::from_slice(labels).unwrap() }
}

/// What one request answered with.
#[derive(Debug, PartialEq, Eq)]
enum Answered {
    Info { sectors: u64, read_only: u32 },
    Data(Vec<u8>),
    Written,
    Flushed,
    Err(ErrorCode),
    Malformed,
}

fn ask(server: &mut Verityd<Fake>, caller: &Caller, request: &Message<'_>) -> Answered {
    ask_with_lend(server, caller, request, LEND)
}

fn ask_with_lend(
    server: &mut Verityd<Fake>,
    caller: &Caller,
    request: &Message<'_>,
    lend: usize,
) -> Answered {
    let mut buf = vec![0u8; LEND];
    let words = request.encode(&mut buf).expect("the request encodes");
    let opcode = redoubt_rt::wire::typed::opcode(&words).unwrap();
    // `flush` is inline: its caller lends nothing.
    if matches!(request, Message::Flush(_)) {
        let outcome = answer_with(server, caller, &words, &ReceivedHandles::new(), &mut []);
        return decode(opcode, &outcome.words, &[]);
    }
    buf.truncate(lend);
    let outcome = answer_with(server, caller, &words, &ReceivedHandles::new(), &mut buf);
    decode(opcode, &outcome.words, &buf)
}

fn decode(opcode: u32, words: &Words, buf: &[u8]) -> Answered {
    if words[0] == u64::from(redoubt_rt::wire::typed::MALFORMED) {
        return Answered::Malformed;
    }
    match Reply::decode(opcode, words, buf, 0) {
        Ok(Ok(Reply::Info(r))) => {
            assert_eq!(r.sector_size, SECTOR);
            Answered::Info { sectors: r.sectors, read_only: r.read_only }
        }
        Ok(Ok(Reply::Read(r))) => Answered::Data(r.data.to_vec()),
        Ok(Ok(Reply::Write(_))) => Answered::Written,
        Ok(Ok(Reply::Flush(_))) => Answered::Flushed,
        Ok(Err(code)) => Answered::Err(code),
        Err(_) => Answered::Malformed,
    }
}

fn read(sector: u64, count: u32) -> Message<'static> { Message::Read(Read { sector, count }) }

/// The sectors `count` from `sector` of the packed bytes.
fn sectors(bytes: &[u8], sector: u64, count: u32) -> Answered {
    let at = sector as usize * SECTOR as usize;
    Answered::Data(bytes[at..at + count as usize * SECTOR as usize].to_vec())
}

fn fsd() -> Caller { caller(BADGE, &[]) }

// ---------------------------------------------------------------- arguments

#[test]
fn arguments_are_inits_and_nothing_else() {
    let root = "ab".repeat(32);
    let args = |extra: &[&str]| {
        let mut all = vec!["endpoint=verity:system"];
        let r = std::format!("root={root}");
        all.push(&r);
        all.push("blocks=129");
        all.extend_from_slice(extra);
        parse_args(all.into_iter())
            .map(|a| (std::string::String::from(a.endpoint), a.labels, a.root, a.geometry.data_blocks()))
    };
    assert_eq!(args(&[]), Ok(("verity:system".into(), vec![], [0xab; 32], 129)));
    assert_eq!(args(&["labels=3,1"]).map(|a| a.1), Ok(vec![3, 1]));
    for bad in [
        "labels=1,1",
        "labels=01",
        "labels=",
        "buckets=4",
        "root=00",
        "blocks=7",
        "endpoint=other",
        "verbose",
    ] {
        assert_eq!(args(&[bad]), Err(BadArgs), "{bad}");
    }
    let parse = |list: &[&str]| parse_args(list.iter().copied()).map(|_| ());
    let r = std::format!("root={root}");
    assert_eq!(parse(&["endpoint=v", &r]), Err(BadArgs), "no blocks");
    assert_eq!(parse(&["endpoint=v", "blocks=1"]), Err(BadArgs), "no root");
    assert_eq!(parse(&[&r, "blocks=1"]), Err(BadArgs), "no endpoint");
    assert_eq!(parse(&["endpoint=v", &r, "blocks=0"]), Err(BadArgs), "no blocks at all");
    assert_eq!(parse(&["endpoint=v", &r, "blocks=18446744073709551615"]), Err(BadArgs), "no tree fits");
    let upper = std::format!("root={}", "AB".repeat(32));
    assert_eq!(parse(&["endpoint=v", &upper, "blocks=1"]), Err(BadArgs), "lowercase only");
    assert_eq!(parse(&["endpoint=v", &r, "blocks=1"]), Ok(()));
}

// ---------------------------------------------------------------- the start check

#[test]
fn a_truncated_range_is_refused_and_still_sized() {
    let (g, mut bytes, root) = packed(129, 0);
    bytes.truncate(bytes.len() - SECTOR as usize);
    let mut s = server(g, bytes, &root, &[]);
    assert_eq!(s.refused(), Some(Refusal::Truncated));
    let line = std::format!("{}", s.take_line().unwrap());
    assert_eq!(line, "verityd: the volume is refused: its range is shorter than its blocks and their tree");
    assert_eq!(s.take_line(), None, "said once");
    // `info` answers what the manifest says, so `fsd` mounts, fails, and serves corrupt.
    assert_eq!(
        ask(&mut s, &fsd(), &Message::Info(Info {})),
        Answered::Info { sectors: 129 * SECTORS_PER_BLOCK, read_only: 1 }
    );
    assert_eq!(ask(&mut s, &fsd(), &read(0, 8)), Answered::Err(ErrorCode::Failed));
}

#[test]
fn a_wrong_root_or_top_is_refused() {
    let (g, bytes, root) = packed(129, 1);
    let mut wrong = root;
    wrong[0] ^= 0x10;
    let mut s = server(g, bytes.clone(), &wrong, &[]);
    assert_eq!(s.refused(), Some(Refusal::Root));
    assert_eq!(ask(&mut s, &fsd(), &read(0, 1)), Answered::Err(ErrorCode::Failed));
    let mut flipped = bytes.clone();
    flipped[g.top() as usize * BLOCK + 100] ^= 1;
    assert_eq!(server(g, flipped, &root, &[]).refused(), Some(Refusal::Root));
    // The root pins the block count: the same disk under a smaller count does not open.
    let fewer = Geometry::new(128).unwrap();
    assert_eq!(server(fewer, bytes.clone(), &root, &[]).refused(), Some(Refusal::Root));
    let mut dead = Verityd::new(Fake { bytes, reads: 0, fail: true }, g, &root, vec![]);
    assert_eq!(dead.refused(), Some(Refusal::NoInfo));
    assert_eq!(
        ask(&mut dead, &fsd(), &Message::Info(Info {})),
        Answered::Info { sectors: 129 * SECTORS_PER_BLOCK, read_only: 1 }
    );
}

// ---------------------------------------------------------------- reading

#[test]
fn sub_block_and_multi_block_reads_return_the_volume() {
    let (g, bytes, root) = packed(300, 2);
    let mut s = server(g, bytes.clone(), &root, &[]);
    assert_eq!(s.refused(), None);
    for (sector, count) in
        [(0, 1), (3, 2), (7, 1), (7, 2), (8, 8), (5, 20), (0, MAX_SECTORS), (300 * SECTORS_PER_BLOCK - 1, 1)]
    {
        assert_eq!(
            ask(&mut s, &fsd(), &read(sector, count)),
            sectors(&bytes, sector, count),
            "{sector}+{count}"
        );
    }
    assert_eq!(
        ask(&mut s, &fsd(), &read(300 * SECTORS_PER_BLOCK - 1, 2)),
        Answered::Err(ErrorCode::OutOfRange),
        "the tree is not the volume's"
    );
    assert_eq!(ask(&mut s, &fsd(), &read(300 * SECTORS_PER_BLOCK, 1)), Answered::Err(ErrorCode::OutOfRange));
    assert_eq!(ask(&mut s, &fsd(), &read(u64::MAX, 1)), Answered::Err(ErrorCode::OutOfRange));
    assert_eq!(ask(&mut s, &fsd(), &read(0, 0)), Answered::Malformed);
    assert_eq!(ask(&mut s, &fsd(), &read(0, MAX_SECTORS + 1)), Answered::Err(ErrorCode::TooMany));
    assert_eq!(
        ask_with_lend(&mut s, &fsd(), &read(0, 8), 4096),
        Answered::Err(ErrorCode::TooMany),
        "no room for the length"
    );
    assert_eq!(s.take_line(), None, "nothing to say about a good volume");
}

/// Sub-block reads within one block hash it once; a level-1 block held serves the blocks it
/// covers without reading the tree again.
#[test]
fn the_last_block_and_the_tree_cache_save_reads() {
    let (g, bytes, root) = packed(129 * 128, 0);
    assert_eq!(g.levels(), 3);
    let mut s = server(g, bytes, &root, &[]);
    for sector in 0..SECTORS_PER_BLOCK {
        ask(&mut s, &fsd(), &read(sector, 1));
    }
    let c = s.counts().unwrap();
    // The top at start, then block 0, its level-1 and level-2 blocks.
    assert_eq!((c.reads, c.checked, c.hits), (4, 1, 0));
    ask(&mut s, &fsd(), &read(SECTORS_PER_BLOCK, 1));
    assert_eq!((s.counts().unwrap().reads, s.counts().unwrap().hits), (5, 1));
    // Block 128 is under another level-1 block, but the same level-2 block.
    ask(&mut s, &fsd(), &read(128 * SECTORS_PER_BLOCK, 1));
    assert_eq!(s.counts().unwrap().reads, 7);
    // More level-1 blocks than the cache holds: the oldest goes, and comes back by a read.
    for k in 0..=TREE_CACHE as u64 {
        ask(&mut s, &fsd(), &read(k * 128 * SECTORS_PER_BLOCK, 1));
    }
    let before = s.counts().unwrap().reads;
    ask(&mut s, &fsd(), &read(2 * SECTORS_PER_BLOCK, 1));
    assert!(s.counts().unwrap().reads >= before + 2, "block 0's level-1 block was evicted");
}

#[test]
fn a_mismatch_is_failed_and_said_naming_the_block() {
    let (g, bytes, root) = packed(129 * 128, 0);
    let (l1, _) = g.node(0, 200).unwrap();
    let (l2, _) = g.node(1, 129 * 128 - 1).unwrap();
    let last = 129 * 128 - 1;
    // Each damage, what is said, and a block whose path crosses it.
    for (at, bad, block) in [
        (200 * BLOCK + 9, Bad::Data(200), 200),
        (l1 as usize * BLOCK + 4095, Bad::Tree(l1), 200),
        (l2 as usize * BLOCK, Bad::Tree(l2), last),
    ] {
        let mut flipped = bytes.clone();
        flipped[at] ^= 0x40;
        let mut s = server(g, flipped, &root, &[]);
        assert_eq!(
            ask(&mut s, &fsd(), &read(block * SECTORS_PER_BLOCK + 1, 1)),
            Answered::Err(ErrorCode::Failed),
            "{bad:?}"
        );
        assert_eq!(s.take_line(), Some(Said::Bad(bad)));
        // Asked again: failed again, and not said again.
        assert_eq!(
            ask(&mut s, &fsd(), &read(block * SECTORS_PER_BLOCK, 8)),
            Answered::Err(ErrorCode::Failed)
        );
        assert_eq!(s.take_line(), None);
        // A block whose path does not cross the damage still reads.
        assert_eq!(ask(&mut s, &fsd(), &read(0, 1)), sectors(&bytes, 0, 1));
    }
    let line = std::format!("{}", Said::Bad(Bad::Tree(16_513)));
    assert_eq!(line, "verityd: tree block 16513 does not match the tree");
    assert_eq!(std::format!("{}", Said::Bad(Bad::Data(7))), "verityd: block 7 does not match the tree");
}

/// A block that failed is never served from the last-block buffer: the next read checks again.
#[test]
fn a_failed_block_is_never_kept() {
    let (g, mut bytes, root) = packed(4, 0);
    bytes[2 * BLOCK] ^= 1;
    let mut s = server(g, bytes, &root, &[]);
    assert_eq!(ask(&mut s, &fsd(), &read(16, 1)), Answered::Err(ErrorCode::Failed));
    assert_eq!(ask(&mut s, &fsd(), &read(17, 1)), Answered::Err(ErrorCode::Failed));
}

// ---------------------------------------------------------------- refusals

#[test]
fn writes_are_refused_and_flush_answers_at_once() {
    let (g, bytes, root) = packed(4, 0);
    let mut s = server(g, bytes, &root, &[]);
    let data = [0u8; 512];
    assert_eq!(
        ask(&mut s, &fsd(), &Message::Write(Write { sector: 0, data: &data })),
        Answered::Err(ErrorCode::NotPermitted)
    );
    assert_eq!(ask(&mut s, &fsd(), &Message::Flush(Flush {})), Answered::Flushed);
}

#[test]
fn the_label_check_runs_on_every_request() {
    let (g, bytes, root) = packed(4, 0);
    let mut s = server(g, bytes.clone(), &root, &[5]);
    let (unlabelled, own, higher) = (caller(BADGE, &[]), caller(BADGE, &[5]), caller(BADGE, &[5, 6]));
    assert_eq!(ask(&mut s, &unlabelled, &read(0, 1)), Answered::Err(ErrorCode::NotPermitted));
    assert_eq!(ask(&mut s, &unlabelled, &Message::Info(Info {})), Answered::Err(ErrorCode::NotPermitted));
    assert_eq!(ask(&mut s, &own, &read(0, 1)), sectors(&bytes, 0, 1));
    assert_eq!(ask(&mut s, &higher, &read(0, 1)), sectors(&bytes, 0, 1), "a read may flow up");
    assert_eq!(ask(&mut s, &higher, &Message::Flush(Flush {})), Answered::Err(ErrorCode::NotPermitted));
    assert_eq!(ask(&mut s, &own, &Message::Flush(Flush {})), Answered::Flushed);
}

#[test]
fn only_the_volumes_badge_is_served() {
    let (g, bytes, root) = packed(4, 0);
    let mut s = server(g, bytes, &root, &[]);
    for badge in [0, 2, u64::MAX] {
        assert_eq!(ask(&mut s, &caller(badge, &[]), &read(0, 1)), Answered::Err(ErrorCode::NotPermitted));
    }
}

// ---------------------------------------------------------------- hostile media

/// Fills `bytes` with xorshift noise from `seed`.
fn noise(seed: &mut u64, bytes: &mut [u8]) {
    for b in bytes {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *b = *seed as u8;
    }
}

/// Noise for a medium, a root that may or may not match, any geometry, and requests anywhere:
/// every answer is an answer, never a panic.
#[test]
fn arbitrary_media_never_panic() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    for round in 0..64u64 {
        let n = 1 + round * 7 % 200;
        let (g, mut bytes, root) = packed(n, (round % 3) as usize);
        let mut r = [0u8; 16];
        noise(&mut seed, &mut r);
        // Damage some rounds' media in a run of bytes, or truncate them.
        if round % 2 == 1 {
            let at = (u64::from_le_bytes(r[..8].try_into().unwrap()) as usize) % bytes.len();
            let end = (at + 1 + r[8] as usize * 64).min(bytes.len());
            noise(&mut seed, &mut bytes[at..end]);
        }
        if round % 5 == 4 {
            bytes.truncate(bytes.len() / 2);
        }
        let mut s = server(g, bytes, &root, &[]);
        for _ in 0..64 {
            let mut q = [0u8; 12];
            noise(&mut seed, &mut q);
            let sector = u64::from_le_bytes(q[..8].try_into().unwrap()) % (n * SECTORS_PER_BLOCK + 16);
            let count = u32::from_le_bytes(q[8..].try_into().unwrap()) % (MAX_SECTORS + 2);
            let _ = ask(&mut s, &fsd(), &read(sector, count));
            let _ = s.take_line();
        }
    }
}
