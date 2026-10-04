//! The 9P server skeleton on hostile request sequences, from several clients (badges, accounts,
//! label sets) against a small labelled tree. Each input is a sequence of requests: most are
//! well-formed messages built from the bytes (so the fuzzer reaches the protocol logic), some are
//! raw bytes; some are `ninep_common`'s `new_connection` and `disconnect`, and later requests
//! come through the connections they minted. The file server refuses some grants.
//! Checked: nothing panics; every reply decodes; the server is never handed a bad walk name, and
//! never sees more than `MAX_FIDS` fids on a connection; no bucket holds more than its limits;
//! a minted badge is never minted twice; a stranger's `disconnect` never succeeds.
//! Requests with the top bit of their op set are multiplexed instead (servers/serving.md,
//! "Multiplexed connections") on the caller's session, opened first, with the clock moving so
//! sessions, holds and requests reach their deadlines: sent in the words, or several end to end
//! in a transfer of one or two pages (its length sometimes wrong), so pages are admitted and
//! partly refused; collected without a call, or by a completion call parked through a small
//! kernel of the target's own ([`Kern`]), which the server answers, empties at its hold or ends,
//! and which the target sometimes abandons.
//! Checked as well: every answer frames and decodes; across every collection a tag is answered
//! no more times than it was sent; no bucket holds more of any resource than its cap; and at the
//! end, every session past its bound, nothing is held for a request or a page, every call has
//! been answered once and every page the server took is freed.
#![no_main]

use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::{LazyLock, Mutex};

use libfuzzer_sys::fuzz_target;
use redoubt_rt::abi::{
    BODY_SLOTS, Body as SysBody, Call, Error, Handle, Handles, Labels, MAX_LEND_PAGES, Message as Sys,
    MessageKind, PAGE_SIZE, Pages, RECEIVED_SLOTS, Received, ReceivedBody, ReceivedHandles, ReplyOutcome,
    Return,
};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event};
use redoubt_rt::path;
use redoubt_rt::server::ninep::{
    Answer, COLLECT_WAIT, DMDIR, FIRST_MINTED_BADGE, FileServer, FileStat, IN_WORDS, MAX_FIDS, Minter,
    NineError, NineServer, QTDIR, Qid, Read, Write, collect_words, ninep_common, refuse_malformed,
};
use redoubt_rt::server::{Admission, AdmitKey, Limits, Override, Resource};
use redoubt_rt::wire::ninep::{Body, Message, NOFID, Names, message_size};

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

    /// A write at an odd offset waits (`Write::Wait`): the skeleton must hold it with
    /// its T-message untouched, after every check a write gets.
    fn write_or_wait(
        &mut self,
        caller: &Caller,
        node: &usize,
        offset: u64,
        data: &[u8],
    ) -> Result<Write, NineError> {
        if offset % 2 == 1 {
            return Ok(Write::Wait);
        }
        self.write(caller, node, offset, data).map(Write::Done)
    }

    fn stat(&mut self, _: &Caller, node: &usize) -> Result<FileStat, NineError> {
        Ok(FileStat { length: self.data[*node].len() as u64, ..stat(*node) })
    }

    /// Refuses one grant size, as a server metering bytes may.
    fn minted(&mut self, _: &Caller, _: u64, _: u64, _: &usize, quota: u64) -> Result<(), NineError> {
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

const LIMITS: Limits = Limits { buckets: 6, in_flight: 8, files: 20, state: 6, requests: 12, pages: 4 };

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

/// The kernel the server's system calls reach (redoubt-rt's host `Transport`), for the target's one
/// thread: `receive` hands out what the target queued, `reply` records what each call was
/// answered, pages come from the heap and go back to it, and the clock is the target's.
struct Kern(Mutex<Machine>);

#[derive(Default)]
struct Machine {
    now: u64,
    queue: VecDeque<Received>,
    /// The lend of each call not yet replied to, by message id: address and length.
    lends: HashMap<u64, (usize, usize)>,
    /// Calls the target gave up: their replies reach nobody.
    abandoned: HashSet<u64>,
    /// Each delivered reply: its call's message id, words, and what its lend then held.
    replies: Vec<(u64, [usize; 4], Vec<u8>)>,
    /// Pages mapped, by address: their length.
    mapped: HashMap<usize, usize>,
}

static KERN: LazyLock<Kern> = LazyLock::new(|| Kern(Mutex::new(Machine::default())));

fn machine() -> std::sync::MutexGuard<'static, Machine> { KERN.0.lock().unwrap() }

fn layout(len: usize) -> Layout { Layout::from_size_align(len, PAGE_SIZE).unwrap() }

impl Machine {
    /// `npages` zeroed pages, as `map_anon` gives.
    fn map(&mut self, npages: usize) -> Pages {
        // SAFETY: the layout has a non-zero size.
        let addr = unsafe { alloc_zeroed(layout(npages * PAGE_SIZE)) } as usize;
        assert_ne!(addr, 0, "out of memory");
        self.mapped.insert(addr, npages * PAGE_SIZE);
        Pages { addr, npages: NonZeroUsize::new(npages).unwrap() }
    }

    fn unmap(&mut self, addr: usize) -> Result<(), Error> {
        let len = self.mapped.remove(&addr).ok_or(Error::InvalidArgument)?;
        // SAFETY: allocated by `map` with this layout, and no longer mapped.
        unsafe { dealloc(addr as *mut u8, layout(len)) };
        Ok(())
    }
}

// SAFETY: every address it returns is its own allocation, live until unmapped; it reads and writes
// records only at the addresses the runtime passes for the call, as the kernel would.
unsafe impl redoubt_rt::Transport for Kern {
    fn call(&self, call: &Call) -> Result<Return, Error> {
        let mut m = machine();
        match *call {
            Call::MapAnon { len, .. } if len > 0 && len % PAGE_SIZE == 0 => {
                Ok(Return::Addr(m.map(len / PAGE_SIZE).addr))
            }
            Call::Unmap { addr, .. } => m.unmap(addr).map(|()| Return::Nothing),
            Call::Receive { received_rec, .. } => {
                let next = m.queue.pop_front().ok_or(Error::Timeout)?;
                // SAFETY: the runtime's live, 8-aligned receive record, borrowed mutably for the call.
                unsafe { (received_rec as *mut [u64; RECEIVED_SLOTS]).write(next.encode()) };
                Ok(Return::Nothing)
            }
            Call::Reply { msg_id, body_rec } => {
                // SAFETY: the runtime's live, 8-aligned body record, for the call.
                let body = SysBody::decode(&unsafe { (body_rec as *const [u64; BODY_SLOTS]).read() })?;
                let (addr, len) = m.lends.remove(&msg_id.get()).ok_or(Error::InvalidArgument)?;
                let delivered = !m.abandoned.remove(&msg_id.get());
                if delivered {
                    let n = body.words[1].min(len);
                    // SAFETY: the lend is this kernel's mapping of `len` bytes, live until unmapped below.
                    let bytes = unsafe { std::slice::from_raw_parts(addr as *const u8, n) }.to_vec();
                    m.replies.push((msg_id.get(), body.words, bytes));
                }
                // The lend goes back to its caller, the target, which is done with it.
                m.unmap(addr)?;
                Ok(Return::Reply(ReplyOutcome { delivered, installed: 0 }))
            }
            Call::Serve { .. } | Call::HandleClose { .. } => Ok(Return::Nothing),
            Call::TimeNow => Ok(Return::Time(m.now)),
            _ => Err(Error::InvalidArgument),
        }
    }
}

/// A message from `caller` the server's next `receive` takes: a call lending `pages`, or a send
/// transferring them.
fn queue(caller: &Caller, words: [usize; 4], call: bool, pages: Option<Pages>, id: u64) {
    let kind = if call { MessageKind::Call { lend: pages } } else { MessageKind::Send { transfer: pages } };
    let body = ReceivedBody { words, handles: ReceivedHandles::from_slice(&[]).unwrap() };
    let (badge, account, labels) = (caller.badge, caller.account, caller.labels);
    let msg_id = NonZeroU64::new(id).unwrap();
    machine().queue.push_back(Received::Message(Sys { kind, msg_id, badge, account, labels, body }));
}

/// What the server's next `receive` returns.
fn receive() -> Event { Endpoint::from_handle(Handle::new(1).unwrap()).receive(0, MAX_LEND_PAGES).unwrap() }

/// A multiplexed connection, as the server keys it, and a tag on it.
type Tag = (u64, AdmitKey, u16);

/// What the client side has seen: each tag's sends less its answers, the completion calls out,
/// and the next message id.
#[derive(Default)]
struct Client {
    owed: Vec<(Tag, u32)>,
    calls: HashMap<u64, Caller>,
    next_id: u64,
}

impl Client {
    fn sent(&mut self, server: &NineServer<Tree>, caller: &Caller, message: &[u8]) {
        if let Some(tag) = message.get(5..7) {
            let key = (caller.badge, server.charge_of(caller).0, u16::from_le_bytes([tag[0], tag[1]]));
            match self.owed.iter_mut().find(|(k, _)| *k == key) {
                Some((_, n)) => *n += 1,
                None => self.owed.push((key, 1)),
            }
        }
    }

    /// The answers in `bytes`, to `caller`'s connection: each frames and decodes, and answers a
    /// tag sent more times than it was answered before.
    fn answered(&mut self, server: &NineServer<Tree>, caller: &Caller, bytes: &[u8]) {
        let mut at = 0;
        while at < bytes.len() {
            let size = message_size(&bytes[at..]).expect("every answer frames");
            let answer = Message::decode(&bytes[at..at + size]).expect("every answer decodes");
            let key = (caller.badge, server.charge_of(caller).0, answer.tag);
            let owed = self.owed.iter_mut().find(|(k, n)| *k == key && *n > 0);
            owed.expect("a tag answered more times than it was sent").1 -= 1;
            at += size;
        }
    }

    /// Every completion call the kernel saw answered since.
    fn replies(&mut self, server: &NineServer<Tree>) {
        let replies = std::mem::take(&mut machine().replies);
        for (id, words, bytes) in replies {
            let caller = self.calls.remove(&id).expect("a reply to a call the target made");
            if words[0] == 0 {
                self.answered(server, &caller, &bytes);
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    redoubt_rt::install_transport(&*KERN);
    *machine() = Machine::default();
    // Badge 1, account 0, has its own caps (an override), no larger than the default
    // so the checks below hold for every bucket.
    let overrides = [Override { badge: 1, in_flight: 0, files: 10, state: 3 }];
    let admission = Admission::with_overrides(LIMITS, &overrides).unwrap();
    let mut server = NineServer::with_admission(Tree { data: Default::default() }, admission, 0);
    let mut kernel = Kernel { minted: Vec::new(), rng: 0x2545_f491_4f6c_dd1d };
    // Connection ids handed out, with who asked for each.
    let mut ids: Vec<(u64, Caller)> = Vec::new();
    let mut input = Input(data);
    let mut lend = vec![0u8; 8192];
    let mut now = 0u64;
    let mut client = Client { next_id: 1, ..Client::default() };
    // Everyone who sent or collected, for the check at the end.
    let mut callers: Vec<Caller> = Vec::new();
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
        // Up to 9 s a step: sessions and requests (10 s) reach their deadlines.
        now += u64::from(arg % 4) * 3_000_000;
        machine().now = now;
        server.expire(now);
        client.replies(&server);
        let names: Vec<&str> =
            (0..(arg >> 8) % 5).map(|i| NAMES[((arg >> (12 + 3 * i)) % 8) as usize]).collect();
        let body = match op % 15 {
            14 => {
                // A connection rooted at a node the file server chose (`ipd`'s grant), kept or
                // undone as an undelivered reply would be (servers/serving.md, "Replies and
                // rollback").
                let root = arg as usize % TREE.len();
                if let Ok((_, id, badge)) = server.mint_rooted(&caller, (root, qid(root)), &mut kernel) {
                    if arg & 0x100 == 0 {
                        ids.push((id, caller));
                    } else {
                        server.unmint(badge);
                    }
                }
                continue;
            }
            12 => {
                let root = ["", "a", "a/b", "..", "vault", "a/b/f", "nope"][arg as usize % 7];
                let quota = [0, 1, 1000, u64::MAX][(arg >> 4) as usize % 4];
                let message =
                    ninep_common::Message::NewConnection(ninep_common::NewConnection { root, quota });
                let words = message.encode(&mut lend).unwrap();
                let outcome = server.answer_common(&caller, &words, &Handles::new(), &mut lend, &mut kernel);
                if outcome.words[0] == 0 {
                    let reply =
                        ninep_common::Reply::decode(2, &outcome.words, &lend, 1).expect("the reply decodes");
                    let Ok(ninep_common::Reply::NewConnection(reply)) = reply else { panic!("{reply:?}") };
                    ids.push((reply.id, caller));
                }
                continue;
            }
            13 => {
                let (id, owner) =
                    ids.get(fid as usize % (ids.len() + 1)).copied().unwrap_or((u64::from(arg), caller));
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
        if op & 0x80 != 0 {
            multiplexed(&mut server, &mut client, &caller, lend, arg, now);
            client.replies(&server);
            callers.push(caller);
            check(&server, &caller);
            continue;
        }
        // A write at an odd offset may be held; then its request must be exactly as it came.
        let before = lend.to_vec();
        match server.answer_in_place(&caller, lend) {
            Answer::Replied => {
                Message::decode(lend).expect("every reply decodes");
            }
            Answer::Waiting => {
                assert_eq!(&lend[..], &before[..], "a held request was changed");
                let Ok(Message { body: Body::Twrite { offset, .. }, .. }) = Message::decode(lend) else {
                    panic!("something other than a write was held");
                };
                assert_eq!(offset % 2, 1);
            }
            Answer::NoRoom => {}
        }
        check(&server, &caller);
    }
    // Every session past its bound: a parked completion call is answered empty at its hold, and a
    // session with none ends a bound later, with everything it held.
    for _ in 0..2 {
        now += COLLECT_WAIT + 1;
        machine().now = now;
        server.expire(now);
    }
    client.replies(&server);
    assert_eq!(server.sessions(), 0, "a session outlived its bound");
    for caller in &callers {
        let key = server.charge_of(caller).0;
        assert_eq!(server.admission().held(key, Resource::Requests), 0, "a request outlived its session");
        assert_eq!(server.admission().held(key, Resource::Pages), 0, "a page outlived its requests");
    }
    drop(server);
    let m = machine();
    assert!(m.lends.is_empty(), "a completion call never answered");
    assert!(m.mapped.is_empty(), "pages the server took never freed");
});

/// What holds after every step: no connection over `MAX_FIDS`, no bucket over a cap.
fn check(server: &NineServer<Tree>, caller: &Caller) {
    assert!(server.fids(caller) <= MAX_FIDS);
    let admission = server.admission();
    for key in [AdmitKey::of(caller), server.charge_of(caller).0] {
        assert!(admission.held(key, Resource::Files) <= LIMITS.files);
        assert!(admission.held(key, Resource::State) <= LIMITS.state);
        assert!(admission.held(key, Resource::InFlight) <= LIMITS.in_flight);
        assert!(admission.held(key, Resource::Requests) <= LIMITS.requests);
        assert!(admission.held(key, Resource::Pages) <= LIMITS.pages);
    }
    assert!(admission.keys() <= LIMITS.buckets as usize);
    assert!(server.connections() <= (LIMITS.buckets * LIMITS.state) as usize);
}

/// The request at the front of `lend`, multiplexed on `caller`'s session (opened first if it is
/// not), as `arg` picks: sent in the words, or copies of it, retagged, end to end in a transfer;
/// then collected without a call, by a completion call parked, or neither; or one of `caller`'s
/// completion calls abandoned, or every waiting request served again.
fn multiplexed(
    server: &mut NineServer<Tree>,
    client: &mut Client,
    caller: &Caller,
    lend: &mut [u8],
    arg: u32,
    now: u64,
) {
    let _ = server.open_session(caller, now);
    let len = message_size(lend).unwrap_or(IN_WORDS).min(lend.len());
    match (arg >> 9) % 4 {
        0 => {
            let mut words = [0u64; 4];
            for (word, chunk) in words[1..].iter_mut().zip(lend[..IN_WORDS].chunks(8)) {
                *word = u64::from_le_bytes(chunk.try_into().unwrap());
            }
            client.sent(server, caller, &lend[..IN_WORDS]);
            server.take_request(caller, &words, now);
        }
        1 => {
            let npages = 1 + (arg >> 11) as usize % 2;
            let copies = (1 + (arg >> 12) as usize % 8).min(npages * PAGE_SIZE / len.max(1));
            if copies == 0 {
                return;
            }
            let pages = machine().map(npages);
            // SAFETY: `map` just gave these `npages` pages, which nothing else holds.
            let bytes = unsafe { std::slice::from_raw_parts_mut(pages.addr as *mut u8, npages * PAGE_SIZE) };
            for i in 0..copies {
                let at = i * len;
                bytes[at..at + len].copy_from_slice(&lend[..len]);
                if len >= 7 {
                    let tag = u16::from_le_bytes([lend[5], lend[6]]).wrapping_add(i as u16);
                    bytes[at + 5..at + 7].copy_from_slice(&tag.to_le_bytes());
                }
                client.sent(server, caller, &bytes[at..at + len]);
            }
            // Its length right, or off by a few bytes either way.
            let total = (copies * len).saturating_add((arg >> 15) as usize % 5).saturating_sub(2);
            let id = client.next_id;
            client.next_id += 1;
            queue(caller, [0, total, 0, 0], false, Some(pages), id);
            let Event::Send(delivery) = receive() else { panic!("not the send queued") };
            assert!(server.deliver(delivery, now).is_none(), "a request handed back");
        }
        2 => {}
        _ => {
            match (arg >> 11) % 3 {
                // Abandoned: the kernel's notice, and a reply that reaches nobody.
                0 => {
                    let Some(id) = client.calls.iter().find(|(_, c)| *c == caller).map(|(id, _)| *id) else {
                        return;
                    };
                    machine().abandoned.insert(id);
                    if !server.abandoned(NonZeroU64::new(id).unwrap()) {
                        // Answered already, at its hold: nothing to abandon.
                        machine().abandoned.remove(&id);
                    }
                }
                1 => server.wake(now),
                _ => {}
            }
            return;
        }
    }
    if arg & 0x8000_0000 != 0 {
        // A completion call parked, held as `arg` says; its lend one page or two.
        let hold = [0, 1_000, 3_000_000, 20_000_000][(arg >> 13) as usize % 4];
        let npages = 1 + (arg >> 16) as usize % 2;
        let pages = machine().map(npages);
        let id = client.next_id;
        client.next_id += 1;
        machine().lends.insert(id, (pages.addr, npages * PAGE_SIZE));
        client.calls.insert(id, *caller);
        let words = collect_words(hold).map(|w| w as usize);
        queue(caller, words, true, Some(pages), id);
        let Event::Call(request) = receive() else { panic!("not the call queued") };
        let _ = server.serve_with(request, |_, r| refuse_malformed(r));
    } else if arg & 0x4000_0000 != 0 {
        let n = server.collect_into(caller, lend, now);
        client.answered(server, caller, &lend[..n]);
    }
}
