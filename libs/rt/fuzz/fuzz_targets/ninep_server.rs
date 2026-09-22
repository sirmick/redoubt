//! The 9P server skeleton on hostile request sequences, from several clients (badges, accounts,
//! label sets) against a small labelled tree. Each input is a sequence of requests: most are
//! well-formed messages built from the bytes (so the fuzzer reaches the protocol logic), some are
//! raw bytes; some are `ninep_common`'s `new_connection` and `disconnect`, and later requests
//! come through the connections they minted. The file server refuses some grants.
//! Checked: nothing panics; every reply decodes; the server is never handed a bad walk name, and
//! never sees more than `MAX_FIDS` fids on a connection; no bucket holds more than its limits;
//! a minted badge is never minted twice; a stranger's `disconnect` never succeeds.
#![no_main]

use std::num::NonZeroU64;

use libfuzzer_sys::fuzz_target;
use redoubt_rt::abi::{Error, Handle, Handles, Labels};
use redoubt_rt::ipc::Caller;
use redoubt_rt::path;
use redoubt_rt::server::ninep::{
    Answer, DMDIR, FIRST_MINTED_BADGE, FileServer, FileStat, MAX_FIDS, Minter, NineError, NineServer,
    QTDIR, Qid, Read, ninep_common,
};
use redoubt_rt::server::{AdmitKey, Limits, Resource};
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

    fn read(&mut self, _: &Caller, node: &usize, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        let data = &self.data[*node];
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(data.len());
        let n = out.len().min(data.len() - start);
        out[..n].copy_from_slice(&data[start..start + n]);
        Ok(Read::Done(n))
    }

    fn write(&mut self, _: &Caller, node: &usize, offset: u64, data: &[u8]) -> Result<usize, NineError> {
        let offset = usize::try_from(offset).ok().filter(|o| *o < 4096).ok_or(NineError::BAD_OFFSET)?;
        let file = &mut self.data[*node];
        file.resize(file.len().max(offset + data.len()).min(8192), 0);
        let n = data.len().min(file.len() - offset);
        file[offset..offset + n].copy_from_slice(&data[..n]);
        Ok(n)
    }

    fn stat(&mut self, _: &Caller, node: &usize) -> Result<FileStat, NineError> {
        Ok(FileStat { length: self.data[*node].len() as u64, ..stat(*node) })
    }

    /// Refuses one grant size, as a server metering bytes may.
    fn minted(&mut self, _: &Caller, _: u64, _: &usize, quota: u64) -> Result<(), NineError> {
        if quota == 1 { Err(NineError("quota refused")) } else { Ok(()) }
    }

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

const LIMITS: Limits = Limits { buckets: 6, in_flight: 0, files: 20, state: 6 };

/// Mints handles by number and draws ids from a xorshift; remembers every badge minted.
struct Kernel {
    minted: Vec<u64>,
    rng: u64,
}

impl Minter for Kernel {
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
        assert!(badge.get() >= FIRST_MINTED_BADGE && !self.minted.contains(&badge.get()), "badge reused");
        self.minted.push(badge.get());
        Ok(Handle::new(1).unwrap())
    }

    fn random(&mut self) -> Result<u64, Error> {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        Ok(self.rng)
    }
}

fuzz_target!(|data: &[u8]| {
    let mut server = NineServer::new(Tree { data: Default::default() }, LIMITS, 0).unwrap();
    let mut kernel = Kernel { minted: Vec::new(), rng: 0x2545_f491_4f6c_dd1d };
    // Connection ids handed out, with who asked for each.
    let mut ids: Vec<(u64, Caller)> = Vec::new();
    let mut input = Input(data);
    let mut lend = vec![0u8; 8192];
    while let Some(op) = input.byte() {
        let who = input.byte().unwrap_or(0);
        let labels: &[u64] = [&[][..], &[7], &[7, 8], &[8]][(who >> 2) as usize % 4];
        // The server's own badges 0-2, or one it minted (or never minted).
        let badge = match who & 3 {
            3 if !kernel.minted.is_empty() => kernel.minted[usize::from(who >> 4) % kernel.minted.len()],
            3 => FIRST_MINTED_BADGE + 5,
            small => u64::from(small),
        };
        let caller =
            Caller { badge, account: u64::from(who >> 6), labels: Labels::from_slice(labels).unwrap() };
        let fid = u32::from(input.byte().unwrap_or(0) % 70);
        let arg = input.word().unwrap_or(0);
        let room = 64 + (arg as usize % 8000);
        let names: Vec<&str> =
            (0..(arg >> 8) % 5).map(|i| NAMES[((arg >> (12 + 3 * i)) % 8) as usize]).collect();
        let body = match op % 14 {
            12 => {
                let root = ["", "a", "a/b", "..", "vault", "a/b/f", "nope"][arg as usize % 7];
                let quota = [0, 1, 1000, u64::MAX][(arg >> 4) as usize % 4];
                let message = ninep_common::Message::NewConnection(ninep_common::NewConnection { root, quota });
                let words = message.encode(&mut lend).unwrap();
                let outcome = server.answer_common(&caller, &words, &Handles::new(), &mut lend, &mut kernel);
                if outcome.words[0] == 0 {
                    let reply = ninep_common::Reply::decode(2, &outcome.words, &lend, 1).expect("the reply decodes");
                    let Ok(ninep_common::Reply::NewConnection(reply)) = reply else { panic!("{reply:?}") };
                    ids.push((reply.id, caller));
                }
                continue;
            }
            13 => {
                let (id, owner) = ids.get(fid as usize % (ids.len() + 1)).copied().unwrap_or((u64::from(arg), caller));
                let message = ninep_common::Message::Disconnect(ninep_common::Disconnect { id });
                let words = message.encode(&mut []).unwrap();
                let outcome = server.answer_common(&caller, &words, &Handles::new(), &mut [], &mut kernel);
                if outcome.words[0] == 0 {
                    // Only the connection that asked for the id, in the same account and labels.
                    assert_eq!((owner.badge, AdmitKey::of(&owner)), (caller.badge, AdmitKey::of(&caller)));
                    ids.retain(|(i, _)| *i != id);
                }
                continue;
            }
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
        // This tree never waits, so every request is answered.
        if server.answer_in_place(&caller, lend) == Answer::Replied {
            Message::decode(lend).expect("every reply decodes");
        }
        assert!(server.fids(&caller) <= MAX_FIDS);
        let admission = server.admission();
        let key = AdmitKey::of(&caller);
        assert!(admission.held(key, Resource::Files) <= LIMITS.files);
        assert!(admission.held(key, Resource::State) <= LIMITS.state);
        assert!(admission.keys() <= LIMITS.buckets as usize);
        assert!(server.connections() <= (LIMITS.buckets * LIMITS.state) as usize);
    }
});
