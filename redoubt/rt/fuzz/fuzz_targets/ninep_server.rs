//! The 9P server skeleton on hostile request sequences, from several clients (badges, accounts,
//! label sets) against a small labelled tree. Each input is a sequence of requests: most are
//! well-formed messages built from the bytes (so the fuzzer reaches the protocol logic), some are
//! raw bytes. Checked: nothing panics; every reply decodes; the server is never handed a bad
//! walk name, and never sees more than `MAX_FIDS` fids on a connection.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_rt::abi::Labels;
use redoubt_rt::ipc::Caller;
use redoubt_rt::path;
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{DMDIR, FileServer, FileStat, MAX_FIDS, NineError, NineServer, QTDIR, Qid};
use redoubt_rt::wire::ninep::{Body, Message, NOFID, Names};

/// name, parent, directory, labels
const TREE: [(&str, usize, bool, &[u64]); 7] = [
    ("", 0, true, &[]),
    ("a", 0, true, &[]),
    ("b", 1, true, &[]),
    ("f", 2, false, &[]),
    ("vault", 0, true, &[7]),
    ("key", 4, false, &[7]),
    ("both", 0, false, &[7, 8]),
];

struct Tree {
    data: [Vec<u8>; TREE.len()],
}

fn qid(n: usize) -> Qid { Qid { kind: if TREE[n].2 { QTDIR } else { 0 }, version: 0, path: n as u64 } }

fn stat(n: usize) -> FileStat {
    FileStat {
        qid: qid(n),
        mode: if TREE[n].2 { DMDIR } else { 0o644 },
        mtime: 0,
        length: 0,
        name: TREE[n].0.into(),
    }
}

impl FileServer for Tree {
    type Node = usize;

    fn attach(&mut self, _: &Caller, aname: &str) -> Result<(usize, Qid), NineError> {
        let root =
            TREE.iter().position(|(name, _, dir, _)| *dir && *name == aname).ok_or(NineError::NOT_FOUND)?;
        Ok((root, qid(root)))
    }

    fn labels(&self, node: &usize) -> &[u64] { TREE[*node].3 }

    fn walk(&mut self, _: &Caller, dir: &usize, name: &str) -> Result<(usize, Qid), NineError> {
        assert!(path::valid_name(name) && TREE[*dir].2, "the skeleton passed {name:?} from {dir}");
        let n =
            (1..TREE.len()).find(|&i| TREE[i].1 == *dir && TREE[i].0 == name).ok_or(NineError::NOT_FOUND)?;
        Ok((n, qid(n)))
    }

    fn open(&mut self, _: &Caller, node: &usize, _: u8) -> Result<Qid, NineError> { Ok(qid(*node)) }

    fn read(&mut self, _: &Caller, node: &usize, offset: u64, out: &mut [u8]) -> Result<usize, NineError> {
        let data = &self.data[*node];
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(data.len());
        let n = out.len().min(data.len() - start);
        out[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn write(&mut self, _: &Caller, node: &usize, offset: u64, data: &[u8]) -> Result<usize, NineError> {
        let offset = usize::try_from(offset).ok().filter(|o| *o < 4096).ok_or(NineError::BAD_OFFSET)?;
        let file = &mut self.data[*node];
        file.resize(file.len().max(offset + data.len()).min(8192), 0);
        let n = data.len().min(file.len() - offset);
        file[offset..offset + n].copy_from_slice(&data[..n]);
        Ok(n)
    }

    fn stat(&mut self, _: &Caller, node: &usize) -> Result<FileStat, NineError> { Ok(stat(*node)) }

    fn dir_entry(
        &mut self,
        _: &Caller,
        dir: &usize,
        index: u64,
    ) -> Result<Option<(usize, FileStat)>, NineError> {
        let n = (1..TREE.len()).filter(|&i| TREE[i].1 == *dir).nth(index as usize);
        Ok(n.map(|n| (n, stat(n))))
    }
}

/// Reads the input from the front.
struct Input<'a>(&'a [u8]);

impl Input<'_> {
    fn byte(&mut self) -> Option<u8> {
        let (first, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(*first)
    }

    fn word(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes([self.byte()?, self.byte()?, self.byte()?, self.byte()?]))
    }

    fn take(&mut self, n: usize) -> &[u8] {
        let n = n.min(self.0.len());
        let (taken, rest) = self.0.split_at(n);
        self.0 = rest;
        taken
    }
}

const NAMES: [&str; 8] = ["a", "b", "f", "..", "vault", "key", "", "x/y"];

fuzz_target!(|data: &[u8]| {
    let mut server =
        NineServer::new(Tree { data: Default::default() }, Limits { in_flight: 1, files: 20, state: 1 });
    let mut input = Input(data);
    let mut lend = vec![0u8; 8192];
    while let Some(op) = input.byte() {
        let who = input.byte().unwrap_or(0);
        let labels: &[u64] = [&[][..], &[7], &[7, 8], &[8]][(who >> 2) as usize % 4];
        let caller = Caller {
            badge: u64::from(who & 3),
            account: u64::from(who >> 6),
            labels: Labels::from_slice(labels).unwrap(),
        };
        let fid = u32::from(input.byte().unwrap_or(0) % 70);
        let arg = input.word().unwrap_or(0);
        let room = 64 + (arg as usize % 8000);
        let names: Vec<&str> =
            (0..(arg >> 8) % 5).map(|i| NAMES[((arg >> (12 + 3 * i)) % 8) as usize]).collect();
        let body = match op % 12 {
            0 => {
                // Raw bytes, with a plausible size field half the time.
                let len = (arg as usize >> 4) % 300;
                let raw = input.take(len).to_vec();
                lend[..raw.len()].copy_from_slice(&raw);
                if arg & 1 == 0 && raw.len() >= 7 {
                    lend[..4].copy_from_slice(&(raw.len() as u32).to_le_bytes());
                }
                None
            }
            1 => Some(Body::Tversion { msize: arg, version: if arg & 1 == 0 { "9P2000" } else { "x" } }),
            2 => Some(Body::Tattach {
                fid,
                afid: if arg & 1 == 0 { NOFID } else { 0 },
                uname: "",
                aname: ["", "a", "vault"][arg as usize % 3],
            }),
            3 => Some(Body::Twalk { fid, newfid: (arg >> 24) % 70, wnames: Names::new(&names).unwrap() }),
            4 => Some(Body::Topen { fid, mode: arg as u8 }),
            5 => Some(Body::Tread { fid, offset: u64::from(arg >> 4), count: arg }),
            6 => Some(Body::Twrite {
                fid,
                offset: u64::from(arg >> 20),
                data: input.take((arg % 64) as usize),
            }),
            7 => Some(Body::Tclunk { fid }),
            8 => Some(Body::Tstat { fid }),
            9 => Some(Body::Tremove { fid }),
            10 => {
                Some(Body::Tcreate { fid, name: NAMES[arg as usize % 8], perm: arg, mode: (arg >> 8) as u8 })
            }
            _ => Some(Body::Tflush { oldtag: arg as u16 }),
        };
        let lend = &mut lend[..room];
        if let Some(body) = body {
            if (Message { tag: arg as u16, body }).encode(lend).is_err() {
                continue;
            }
        }
        if server.answer_in_place(&caller, lend).is_some() {
            Message::decode(lend).expect("every reply decodes");
        }
        assert!(server.fids(&caller) <= MAX_FIDS);
        if op % 97 == 96 {
            server.badge_closed(caller.badge);
        }
    }
});
