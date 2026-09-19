//! The 9P skeleton against an in-memory file server, driven through `answer_in_place` (no
//! system calls): hostile fids, names, offsets, counts and modes, and the label check.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use redoubt_sys::Labels;
use redoubt_wire::ninep::{Body, IOHDRSZ, Message, NOFID, Names, Qid};

use super::*;

/// A tree of files, each with its own labels (a real `fsd` has one set per volume).
struct MemFs {
    nodes: Vec<MemNode>,
    /// Every node clunked, in order.
    clunked: Vec<usize>,
    /// Calls to `attach` and `walk`: the work a request made the server do.
    attaches: usize,
    walks: usize,
}

struct MemNode {
    name: String,
    parent: usize,
    dir: bool,
    data: Vec<u8>,
    labels: Vec<u64>,
    removed: bool,
}

impl MemFs {
    /// `/`, `/a/`, `/a/b/`, `/a/b/f` ("deep"), `/notes` ("hello, world"), `/vault/` and
    /// `/vault/key` (labelled 7), and `/secret`: a labelled file in the unlabelled root.
    fn new() -> MemFs {
        let mut fs = MemFs { nodes: Vec::new(), clunked: Vec::new(), attaches: 0, walks: 0 };
        fs.add("", 0, true, b"", &[]);
        let a = fs.add("a", 0, true, b"", &[]);
        let b = fs.add("b", a, true, b"", &[]);
        fs.add("f", b, false, b"deep", &[]);
        fs.add("notes", 0, false, b"hello, world", &[]);
        let vault = fs.add("vault", 0, true, b"", &[7]);
        fs.add("key", vault, false, b"secret", &[7]);
        fs.add("secret", 0, false, b"top secret", &[7]);
        fs
    }

    fn add(&mut self, name: &str, parent: usize, dir: bool, data: &[u8], labels: &[u64]) -> usize {
        let node = MemNode {
            name: name.into(),
            parent,
            dir,
            data: data.to_vec(),
            labels: labels.to_vec(),
            removed: false,
        };
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn qid(&self, n: usize) -> Qid {
        Qid { kind: if self.nodes[n].dir { QTDIR } else { 0 }, version: 0, path: n as u64 }
    }

    fn children(&self, dir: usize) -> impl Iterator<Item = usize> + '_ {
        (1..self.nodes.len()).filter(move |&i| self.nodes[i].parent == dir && !self.nodes[i].removed)
    }

    fn stat_of(&self, n: usize) -> FileStat {
        let node = &self.nodes[n];
        let mode = if node.dir { DMDIR | 0o755 } else { 0o644 };
        FileStat { qid: self.qid(n), mode, mtime: 0, length: node.data.len() as u64, name: node.name.clone() }
    }
}

impl FileServer for MemFs {
    type Node = usize;

    fn attach(&mut self, _: &Caller, aname: &str) -> Result<(usize, Qid), NineError> {
        self.attaches += 1;
        let root = match aname {
            "" => 0,
            "a" => 1,
            "vault" => 5,
            _ => return Err(NineError::NOT_FOUND),
        };
        Ok((root, self.qid(root)))
    }

    fn labels(&self, node: &usize) -> &[u64] { &self.nodes[*node].labels }

    fn walk(&mut self, _: &Caller, dir: &usize, name: &str) -> Result<(usize, Qid), NineError> {
        assert!(path::valid_name(name), "the skeleton passed {name:?}");
        self.walks += 1;
        let found = self.children(*dir).find(|&i| self.nodes[i].name == name);
        found.map(|n| (n, self.qid(n))).ok_or(NineError::NOT_FOUND)
    }

    fn open(&mut self, _: &Caller, node: &usize, mode: u8) -> Result<Qid, NineError> {
        if mode & mode::OTRUNC != 0 {
            self.nodes[*node].data.clear();
        }
        Ok(self.qid(*node))
    }

    fn read(&mut self, _: &Caller, node: &usize, offset: u64, out: &mut [u8]) -> Result<usize, NineError> {
        let data = &self.nodes[*node].data;
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(data.len());
        let n = out.len().min(data.len() - start);
        out[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn write(&mut self, _: &Caller, node: &usize, offset: u64, data: &[u8]) -> Result<usize, NineError> {
        let offset = usize::try_from(offset).ok().filter(|o| *o <= 1 << 20).ok_or(NineError::BAD_OFFSET)?;
        let file = &mut self.nodes[*node].data;
        if file.len() < offset + data.len() {
            file.resize(offset + data.len(), 0);
        }
        file[offset..offset + data.len()].copy_from_slice(data);
        Ok(data.len())
    }

    fn stat(&mut self, _: &Caller, node: &usize) -> Result<FileStat, NineError> { Ok(self.stat_of(*node)) }

    fn dir_entry(
        &mut self,
        _: &Caller,
        dir: &usize,
        index: u64,
    ) -> Result<Option<(usize, FileStat)>, NineError> {
        let n = self.children(*dir).nth(index as usize);
        Ok(n.map(|n| (n, self.stat_of(n))))
    }

    fn create(
        &mut self,
        _: &Caller,
        dir: &usize,
        name: &str,
        perm: u32,
        _: u8,
    ) -> Result<(usize, Qid), NineError> {
        assert!(path::valid_name(name));
        if self.children(*dir).any(|i| self.nodes[i].name == name) {
            return Err(NineError("file exists"));
        }
        let labels = self.nodes[*dir].labels.clone();
        let n = self.add(name, *dir, perm & DMDIR != 0, b"", &labels);
        Ok((n, self.qid(n)))
    }

    fn remove(&mut self, _: &Caller, node: &usize) -> Result<(), NineError> {
        if *node == 0 || self.children(*node).next().is_some() {
            return Err(NineError("cannot remove"));
        }
        self.nodes[*node].removed = true;
        Ok(())
    }

    fn clunk(&mut self, node: &usize) { self.clunked.push(*node); }

    fn quota(&mut self, badge: u64) -> u64 { if badge == QUOTA_BADGE { QUOTA } else { u64::MAX } }
}

/// The server's own badge whose root has a byte quota, and the quota.
const QUOTA_BADGE: u64 = 7;
const QUOTA: u64 = 100;

struct T {
    server: NineServer<MemFs>,
    buf: Vec<u8>,
}

fn caller(badge: u64, account: u64, labels: &[u64]) -> Caller {
    Caller { badge, account, labels: Labels::from_slice(labels).unwrap() }
}

const ALICE: u64 = 1;

fn alice() -> Caller { caller(ALICE, 1001, &[]) }

impl T {
    fn new() -> T { T::with_limit(1000) }

    /// A server whose buckets may hold `files` fids each; a lone share holds half of that.
    fn with_limit(files: u32) -> T {
        let limits = Limits { buckets: 8, in_flight: 0, files, state: 8 };
        T { server: NineServer::new(MemFs::new(), limits).unwrap(), buf: vec![0; MSIZE] }
    }

    /// Sends `body` as `who` with a lend of `lend` bytes; the reply's body.
    fn rpc_with(&mut self, who: &Caller, lend: usize, body: Body<'_>) -> Body<'_> {
        self.buf = vec![0; lend];
        Message { tag: 5, body }.encode(&mut self.buf).unwrap();
        self.server.answer_in_place(who, &mut self.buf).expect("a reply");
        let reply = Message::decode(&self.buf).unwrap();
        assert_eq!(reply.tag, 5);
        reply.body
    }

    fn rpc(&mut self, who: &Caller, body: Body<'_>) -> Body<'_> { self.rpc_with(who, MSIZE, body) }

    /// The error text, or a panic if the reply is not an error.
    fn err(&mut self, who: &Caller, body: Body<'_>) -> String {
        match self.rpc(who, body) {
            Body::Rerror { ename } => ename.into(),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn attach(&mut self, who: &Caller, fid: u32, aname: &str) {
        let reply = self.rpc(who, Body::Tattach { fid, afid: NOFID, uname: "", aname });
        assert!(matches!(reply, Body::Rattach { .. }), "{reply:?}");
    }

    /// Walks and returns the qid paths walked (an error panics).
    fn walk(&mut self, who: &Caller, fid: u32, newfid: u32, names: &[&str]) -> Vec<u64> {
        match self.rpc(who, Body::Twalk { fid, newfid, wnames: Names::new(names).unwrap() }) {
            Body::Rwalk { qids } => qids.as_slice().iter().map(|q| q.path).collect(),
            other => panic!("walk {names:?}: {other:?}"),
        }
    }

    fn open(&mut self, who: &Caller, fid: u32, mode: u8) -> Result<(), String> {
        match self.rpc(who, Body::Topen { fid, mode }) {
            Body::Ropen { .. } => Ok(()),
            Body::Rerror { ename } => Err(ename.into()),
            other => panic!("{other:?}"),
        }
    }

    fn read(&mut self, who: &Caller, fid: u32, offset: u64, count: u32) -> Result<Vec<u8>, String> {
        match self.rpc(who, Body::Tread { fid, offset, count }) {
            Body::Rread { data } => Ok(data.to_vec()),
            Body::Rerror { ename } => Err(ename.into()),
            other => panic!("{other:?}"),
        }
    }

    fn clunk(&mut self, who: &Caller, fid: u32) {
        assert_eq!(self.rpc(who, Body::Tclunk { fid }), Body::Rclunk);
    }
}

#[test]
fn attach_walk_open_read_write() {
    let mut t = T::new();
    let a = alice();
    t.attach(&a, 0, "");
    assert_eq!(t.walk(&a, 0, 1, &["notes"]), vec![4]);
    t.open(&a, 1, mode::ORDWR).unwrap();
    assert_eq!(t.read(&a, 1, 0, 100).unwrap(), b"hello, world");
    assert_eq!(t.read(&a, 1, 7, 3).unwrap(), b"wor");
    assert_eq!(t.rpc(&a, Body::Twrite { fid: 1, offset: 0, data: b"HELLO" }), Body::Rwrite { count: 5 });
    assert_eq!(t.read(&a, 1, 0, 100).unwrap(), b"HELLO, world");
    match t.rpc(&a, Body::Tstat { fid: 1 }) {
        Body::Rstat { stat } => assert_eq!((stat.name, stat.length), ("notes", 12)),
        other => panic!("{other:?}"),
    }
    t.clunk(&a, 1);
    assert_eq!(t.err(&a, Body::Tread { fid: 1, offset: 0, count: 1 }), "unknown fid");
    // Walking with no names clones the fid.
    assert_eq!(t.walk(&a, 0, 2, &[]), Vec::<u64>::new());
    assert_eq!(t.server.fids(&alice()), 2);
}

#[test]
fn dot_dot_never_leaves_the_attach_root() {
    let mut t = T::new();
    let a = alice();
    // Attached at /a: `..` from the root is the root, however many times.
    t.attach(&a, 0, "a");
    assert_eq!(t.walk(&a, 0, 1, &["..", "..", "..", "b", "f"]), vec![1, 1, 1, 2, 3]);
    assert_eq!(t.walk(&a, 0, 2, &["b", "..", "..", ".."]), vec![2, 1, 1, 1]);
    // A file is not walked from, not even by `..`.
    assert_eq!(
        t.err(&a, Body::Twalk { fid: 1, newfid: 5, wnames: Names::new(&[".."]).unwrap() }),
        "not a directory"
    );
    // From /a/b, `..` is /a and then stays there; `notes` (in /) is out of reach.
    assert_eq!(t.walk(&a, 0, 3, &["b"]), vec![2]);
    // The walk stops short there, and a short walk leaves newfid unused.
    assert_eq!(t.walk(&a, 3, 4, &["..", "..", "notes"]).len(), 2);
    assert_eq!(t.err(&a, Body::Tclunk { fid: 4 }), "unknown fid");
    // The same fid walked in place follows the same rule.
    assert_eq!(t.walk(&a, 3, 3, &["..", "..", "b"]), vec![1, 1, 2]);
}

#[test]
fn walk_names_are_components() {
    let mut t = T::new();
    let a = alice();
    t.attach(&a, 0, "");
    for bad in ["", ".", "a/b", "/", "x\0", "../notes"] {
        let names = Names::new(&[bad]).unwrap();
        let e = t.err(&a, Body::Twalk { fid: 0, newfid: 1, wnames: names });
        assert_eq!(e, "bad file name", "{bad:?}");
        // A bad name after a good one ends the walk there.
        assert_eq!(t.walk(&a, 0, 1, &["a", bad]), vec![1], "{bad:?}");
    }
    // A file cannot be walked from.
    assert_eq!(t.walk(&a, 0, 1, &["notes", "x"]), vec![4]);
    assert_eq!(t.server.fids(&alice()), 1, "no failed walk left a fid");
    // An open fid cannot be walked.
    t.walk(&a, 0, 1, &["notes"]);
    t.open(&a, 1, mode::OREAD).unwrap();
    assert_eq!(t.err(&a, Body::Twalk { fid: 1, newfid: 2, wnames: Names::new(&[]).unwrap() }), "fid is open");
}

#[test]
fn depth_is_bounded() {
    let mut t = T::new();
    let a = alice();
    t.attach(&a, 0, "");
    // Build a chain of directories one level at a time, as deep as allowed.
    t.walk(&a, 0, 1, &[]);
    for level in 0..path::MAX_COMPONENTS {
        let name = alloc::format!("d{level}");
        let reply = t.rpc(&a, Body::Tcreate { fid: 1, name: &name, perm: DMDIR | 0o755, mode: mode::OREAD });
        assert!(matches!(reply, Body::Rcreate { .. }), "level {level}: {reply:?}");
        t.clunk(&a, 1);
        let names: Vec<String> = (0..=level).map(|l| alloc::format!("d{l}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        // Walk from the root in chunks of 16.
        t.walk(&a, 0, 1, &[]);
        for chunk in names.chunks(16) {
            t.walk(&a, 1, 1, chunk);
        }
    }
    assert_eq!(t.err(&a, Body::Tcreate { fid: 1, name: "deeper", perm: 0, mode: 0 }), "path too deep");
}

#[test]
fn fids_are_bounded_per_connection_and_per_account() {
    let mut t = T::with_limit(2 * MAX_FIDS as u32);
    let a = alice();
    t.attach(&a, 0, "");
    for fid in 1..MAX_FIDS as u32 {
        t.walk(&a, 0, fid, &[]);
    }
    assert_eq!(
        t.err(&a, Body::Twalk { fid: 0, newfid: 999, wnames: Names::new(&[]).unwrap() }),
        "too many open files"
    );
    // The same account on another connection has its fair share of the account's limit: a
    // third of it, with alice holding fids too.
    let a2 = caller(2, 1001, &[]);
    for fid in 0..42 {
        t.attach(&a2, fid, "");
    }
    assert_eq!(
        t.err(&a2, Body::Tattach { fid: 42, afid: NOFID, uname: "", aname: "" }),
        "too many open files"
    );
    // Another account is not affected.
    let b = caller(3, 2002, &[]);
    t.attach(&b, 0, "");
    // Clunking gives the charge back; Tversion clunks every fid of the connection.
    t.clunk(&a2, 0);
    t.attach(&a2, 42, "");
    let clunked = t.server.fs.clunked.len();
    assert!(matches!(t.rpc(&a, Body::Tversion { msize: 1 << 20, version: "9P2000" }),
        Body::Rversion { msize, version: "9P2000" } if msize == MSIZE as u32));
    assert_eq!(t.server.fids(&alice()), 0);
    assert_eq!(t.server.fs.clunked.len(), clunked + MAX_FIDS);
    // Alone again in its bucket, a2's share is half of it.
    t.attach(&a2, 43, "");
    t.attach(&a2, 11 + 1000, "");
    let a2_fids = t.server.fids(&a2) as u32;
    assert_eq!(t.server.admission().held(AdmitKey::of(&a2), Resource::Files), a2_fids);
    // Fid numbers are checked: in use, and NOFID.
    assert_eq!(
        t.err(&a2, Body::Tattach { fid: 43, afid: NOFID, uname: "", aname: "" }),
        "fid already in use"
    );
    assert_eq!(
        t.err(&a2, Body::Tattach { fid: NOFID, afid: NOFID, uname: "", aname: "" }),
        "fid already in use"
    );
    assert_eq!(
        t.err(&a2, Body::Tattach { fid: 12, afid: 3, uname: "", aname: "" }),
        "authentication not required"
    );
    // Fids are per connection: another badge cannot use alice's.
    t.attach(&a, 0, "");
    assert_eq!(t.err(&b, Body::Tstat { fid: 1 }), "unknown fid");
}

#[test]
fn labels_are_checked_on_every_request() {
    let mut t = T::new();
    let plain = alice();
    let vault = caller(9, 1001, &[7]);
    // An unlabelled caller cannot walk into what it cannot read: a qid is a read (question 52).
    t.attach(&plain, 0, "");
    for names in [&["vault"][..], &["secret"], &["a", "..", "vault", "key"]] {
        let wnames = Names::new(names).unwrap();
        let reply = t.rpc(&plain, Body::Twalk { fid: 0, newfid: 1, wnames });
        assert!(
            matches!(reply, Body::Rerror { ename: "permission denied" })
                || matches!(reply, Body::Rwalk { .. }),
            "{names:?}: {reply:?}"
        );
        if let Body::Rwalk { qids } = reply {
            // Walked short, never into the vault; the partial walk left no fid.
            assert!(qids.as_slice().iter().all(|q| q.path != 5 && q.path != 6 && q.path != 7), "{names:?}");
        }
        assert_eq!(t.err(&plain, Body::Tclunk { fid: 1 }), "unknown fid");
    }
    // Nor attach to a labelled root.
    assert_eq!(
        t.err(&plain, Body::Tattach { fid: 5, afid: NOFID, uname: "", aname: "vault" }),
        "permission denied"
    );
    // The labelled caller reads it, and may also read what is unlabelled (no read up only).
    t.attach(&vault, 0, "");
    assert_eq!(t.walk(&vault, 0, 1, &["vault", "key"]), vec![5, 6]);
    t.open(&vault, 1, mode::OREAD).unwrap();
    assert_eq!(t.read(&vault, 1, 0, 64).unwrap(), b"secret");
    t.walk(&vault, 0, 2, &["notes"]);
    t.open(&vault, 2, mode::OREAD).unwrap();
    // ... but writes nothing down: no write, truncate, create or remove where it is unlabelled.
    t.walk(&vault, 0, 3, &["notes"]);
    assert_eq!(t.open(&vault, 3, mode::OWRITE), Err("permission denied".into()));
    assert_eq!(t.open(&vault, 3, mode::OREAD | mode::OTRUNC), Err("permission denied".into()));
    assert_eq!(
        t.err(&vault, Body::Tcreate { fid: 0, name: "leak", perm: 0o644, mode: mode::OWRITE }),
        "permission denied"
    );
    assert_eq!(t.err(&vault, Body::Tremove { fid: 3 }), "permission denied");
    assert_eq!(t.server.fs.nodes[4].data, b"hello, world");
    // Writing where its labels are equal is fine.
    t.walk(&vault, 0, 4, &["vault", "key"]);
    t.open(&vault, 4, mode::OWRITE).unwrap();
    assert_eq!(t.rpc(&vault, Body::Twrite { fid: 4, offset: 0, data: b"S" }), Body::Rwrite { count: 1 });
}

#[test]
fn an_unlabelled_caller_cannot_reach_labelled_data_to_destroy_or_probe_it() {
    // Red team: with no read check on walked-into nodes, an unlabelled caller truncated,
    // overwrote and removed /secret, planted files in /vault, and used Tcreate's "file exists"
    // as an existence oracle there. It can no longer get a fid on either.
    let mut t = T::new();
    let plain = alice();
    t.attach(&plain, 0, "");
    for target in ["secret", "vault"] {
        let wnames = Names::new(&[target]).unwrap();
        assert_eq!(t.err(&plain, Body::Twalk { fid: 0, newfid: 1, wnames }), "permission denied");
    }
    assert_eq!(t.server.fs.nodes[7].data, b"top secret");
    assert!(!t.server.fs.nodes[7].removed);
    assert_eq!(t.server.fs.children(5).count(), 1, "nothing planted in the vault");
}

#[test]
fn labelled_metadata_does_not_flow_down() {
    // Red team: a walk's qid and a directory listing's stats showed an unlabelled caller each
    // write to a labelled file (a covert channel out of the vault).
    let mut t = T::new();
    let plain = alice();
    let vault = caller(2, 1001, &[7]);
    t.attach(&plain, 0, "");
    t.attach(&vault, 0, "");
    t.walk(&vault, 0, 1, &["secret"]);
    t.open(&vault, 1, mode::OWRITE).unwrap();
    assert_eq!(t.rpc(&vault, Body::Twrite { fid: 1, offset: 0, data: b"x" }), Body::Rwrite { count: 1 });
    assert_eq!(
        t.err(&plain, Body::Twalk { fid: 0, newfid: 9, wnames: Names::new(&["secret"]).unwrap() }),
        "permission denied"
    );
    // The listing of / leaves out what the caller cannot read.
    t.walk(&plain, 0, 2, &[]);
    t.open(&plain, 2, mode::OREAD).unwrap();
    let listing = t.read(&plain, 2, 0, 8192).unwrap();
    let names: Vec<String> =
        redoubt_wire::ninep::stats(&listing).map(|s| String::from(s.unwrap().name)).collect();
    assert_eq!(names, ["a", "notes"]);
}

#[test]
fn copies_of_one_badge_in_other_accounts_or_label_sets_share_nothing() {
    // Red team: fid tables keyed by badge alone let a holder of a copied handle read, clunk,
    // exhaust and Tversion-wipe another client's fids.
    let mut t = T::new();
    let alice = caller(42, 1001, &[]);
    let others = [caller(42, 2002, &[]), caller(42, 1001, &[7])];
    t.attach(&alice, 0, "");
    t.walk(&alice, 0, 1, &["notes"]);
    t.open(&alice, 1, mode::OREAD).unwrap();
    for other in &others {
        assert_eq!(t.err(other, Body::Tread { fid: 1, offset: 0, count: 5 }), "unknown fid");
        assert_eq!(t.err(other, Body::Tclunk { fid: 1 }), "unknown fid");
        t.attach(other, 0, "");
        for fid in 100..100 + MAX_FIDS as u32 - 1 {
            t.walk(other, 0, fid, &[]);
        }
        let _ = t.rpc(other, Body::Tversion { msize: 8192, version: "9P2000" });
    }
    assert_eq!(t.server.fids(&alice), 2);
    assert_eq!(t.read(&alice, 1, 0, 5).unwrap(), b"hello");
    t.walk(&alice, 0, 7, &[]);
}

#[test]
fn only_the_node_a_fid_rests_on_is_clunked() {
    let mut t = T::new();
    let a = alice();
    t.attach(&a, 0, "");
    // A walk in place moves the fid; the nodes passed are values, dropped without a clunk.
    t.walk(&a, 0, 0, &["a", "b", "f"]);
    t.clunk(&a, 0);
    assert_eq!(t.server.fs.clunked, [3]);
    // Remove and Tversion clunk what the fids rest on, once each.
    t.attach(&a, 0, "");
    t.walk(&a, 0, 1, &["notes"]);
    t.walk(&a, 0, 2, &["a"]);
    assert_eq!(t.rpc(&a, Body::Tremove { fid: 1 }), Body::Rremove);
    let _ = t.rpc(&a, Body::Tversion { msize: 8192, version: "9P2000" });
    assert_eq!(t.server.fs.clunked[..2], [3, 4]);
    let mut rest = t.server.fs.clunked[2..].to_vec();
    rest.sort();
    assert_eq!(rest, [0, 1]);
}

#[test]
fn dot_dot_costs_no_server_work_and_admission_comes_first() {
    // Red team: `..` re-walked from the root (16 of them at depth 64 cost ~1000 server walks),
    // and attach and walk ran before admission refused them.
    let mut t = T::with_limit(8);
    let a = alice();
    t.attach(&a, 0, "");
    t.walk(&a, 0, 1, &["a", "b"]);
    let walks = t.server.fs.walks;
    assert_eq!(t.walk(&a, 1, 2, &[".."; 16]), vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(t.server.fs.walks, walks, "`..` asked the server nothing");
    t.walk(&a, 0, 3, &[]);
    // The account is at its share of 4: nothing more reaches the server.
    let (attaches, walks) = (t.server.fs.attaches, t.server.fs.walks);
    for _ in 0..10 {
        assert_eq!(
            t.err(&a, Body::Tattach { fid: 50, afid: NOFID, uname: "", aname: "" }),
            "too many open files"
        );
        let wnames = Names::new(&["a", "b", "f"]).unwrap();
        assert_eq!(t.err(&a, Body::Twalk { fid: 0, newfid: 51, wnames }), "too many open files");
    }
    assert_eq!((t.server.fs.attaches, t.server.fs.walks), (attaches, walks));
    // A walk in place needs no new fid, so it still works at the limit.
    assert_eq!(t.walk(&a, 3, 3, &["a"]), vec![1]);
}

#[test]
fn a_short_reply_leaves_the_rest_of_the_lend_alone() {
    // Nothing from another client, or from the server's scratch, lands past the reply.
    let mut t = T::new();
    let (alice, bob) = (alice(), caller(2, 2002, &[]));
    t.attach(&alice, 0, "");
    t.walk(&alice, 0, 1, &["notes"]);
    t.open(&alice, 1, mode::OREAD).unwrap();
    t.read(&alice, 1, 0, 100).unwrap();
    let mut lend = vec![0xaa; 4096];
    Message { tag: 1, body: Body::Tattach { fid: 0, afid: NOFID, uname: "", aname: "" } }
        .encode(&mut lend)
        .unwrap();
    t.server.answer_in_place(&bob, &mut lend).unwrap();
    let n = redoubt_wire::ninep::message_size(&lend).unwrap();
    assert!(lend[n..].iter().all(|b| *b == 0xaa));
}

#[test]
fn offsets_counts_and_modes_are_not_trusted() {
    let mut t = T::new();
    let a = alice();
    t.attach(&a, 0, "");
    t.walk(&a, 0, 1, &["notes"]);
    // Not open, or open for the wrong thing.
    assert_eq!(t.read(&a, 1, 0, 1), Err("fid not open for this".into()));
    t.open(&a, 1, mode::OWRITE).unwrap();
    assert_eq!(t.read(&a, 1, 0, 1), Err("fid not open for this".into()));
    assert_eq!(t.open(&a, 1, mode::OREAD), Err("fid is open".into()));
    t.walk(&a, 0, 2, &["notes"]);
    assert_eq!(t.err(&a, Body::Twrite { fid: 2, offset: 0, data: b"x" }), "fid not open for this");
    for bad in [0x04, 0x20, 0x40, 0x80, 0xff] {
        assert_eq!(t.open(&a, 2, bad), Err("bad open mode".into()), "{bad:#x}");
    }
    // Directories open only for reading.
    t.walk(&a, 0, 3, &["a"]);
    for bad in [mode::OWRITE, mode::ORDWR, mode::OREAD | mode::OTRUNC] {
        assert_eq!(t.open(&a, 3, bad), Err("bad open mode".into()));
    }
    t.open(&a, 2, mode::OREAD).unwrap();
    // offset + count must not overflow.
    assert_eq!(t.read(&a, 2, u64::MAX, 2), Err("bad offset".into()));
    assert_eq!(t.err(&a, Body::Twrite { fid: 1, offset: u64::MAX, data: b"xy" }), "bad offset");
    // A huge count is cut to what the lend can carry.
    t.server.fs.nodes[4].data = vec![b'z'; 10_000];
    match t.rpc_with(&a, 1000, Body::Tread { fid: 2, offset: 0, count: u32::MAX }) {
        Body::Rread { data } => assert_eq!(data.len(), 1000 - IOHDRSZ),
        other => panic!("{other:?}"),
    }
    assert_eq!(t.read(&a, 2, 0, u32::MAX).unwrap().len(), 10_000);
    assert_eq!(t.read(&a, 2, 1 << 40, 10).unwrap(), b"");
}

#[test]
fn directory_reads() {
    let mut t = T::new();
    let a = alice();
    t.attach(&a, 0, "");
    t.open(&a, 0, mode::OREAD).unwrap();
    let names = |data: &[u8]| -> Vec<String> {
        redoubt_wire::ninep::stats(data).map(|s| String::from(s.unwrap().name)).collect()
    };
    let all = t.read(&a, 0, 0, 8192).unwrap();
    assert_eq!(names(&all), ["a", "notes"], "labelled entries are left out");
    // Read one entry at a time: each read continues where the last ended.
    let one = t.read(&a, 0, 0, 60).unwrap();
    assert_eq!(names(&one), ["a"]);
    let two = t.read(&a, 0, one.len() as u64, 60).unwrap();
    assert_eq!(names(&two), ["notes"]);
    assert_eq!(t.read(&a, 0, (one.len() + two.len()) as u64, 60).unwrap(), b"");
    // The labelled caller sees every entry.
    let v = caller(9, 1001, &[7]);
    t.attach(&v, 0, "");
    t.open(&v, 0, mode::OREAD).unwrap();
    assert_eq!(names(&t.read(&v, 0, 0, 8192).unwrap()), ["a", "notes", "vault", "secret"]);
    // Any other offset is refused, and too small a count for one entry is an error.
    assert_eq!(t.read(&a, 0, 1, 60), Err("bad offset".into()));
    assert_eq!(t.read(&a, 0, 0, 10), Err("count too small".into()));
}

#[test]
fn create_and_remove() {
    let mut t = T::new();
    let a = alice();
    t.attach(&a, 0, "");
    t.walk(&a, 0, 1, &["a"]);
    assert!(matches!(
        t.rpc(&a, Body::Tcreate { fid: 1, name: "new", perm: 0o644, mode: mode::ORDWR }),
        Body::Rcreate { .. }
    ));
    // The fid is now the new file, open for reading and writing.
    assert_eq!(t.rpc(&a, Body::Twrite { fid: 1, offset: 0, data: b"hi" }), Body::Rwrite { count: 2 });
    assert_eq!(t.read(&a, 1, 0, 10).unwrap(), b"hi");
    for bad in ["", "..", "x/y"] {
        t.walk(&a, 0, 2, &["a"]);
        assert_eq!(t.err(&a, Body::Tcreate { fid: 2, name: bad, perm: 0, mode: 0 }), "bad file name");
        t.clunk(&a, 2);
    }
    // Remove clunks the fid even when it fails.
    t.walk(&a, 0, 2, &["a"]);
    assert_eq!(t.err(&a, Body::Tremove { fid: 2 }), "cannot remove");
    assert_eq!(t.err(&a, Body::Tclunk { fid: 2 }), "unknown fid");
    assert_eq!(t.rpc(&a, Body::Tremove { fid: 1 }), Body::Rremove);
    assert_eq!(t.walk(&a, 0, 3, &["a", "new"]), vec![1], "removed");
}

#[test]
fn malformed_requests_get_errors() {
    let mut t = T::new();
    let a = alice();
    // Garbage with a readable tag gets an Rerror with that tag.
    let mut buf = vec![0u8; 64];
    buf[..7].copy_from_slice(&[64, 0, 0, 0, 99, 0x34, 0x12]);
    assert_eq!(t.server.answer_in_place(&a, &mut buf), Some(()));
    let reply = Message::decode(&buf).unwrap();
    assert_eq!((reply.tag, reply.body), (0x1234, Body::Rerror { ename: "malformed message" }));
    // R-messages and unsupported requests.
    assert_eq!(t.err(&a, Body::Rclunk), "malformed message");
    assert_eq!(t.err(&a, Body::Tauth { afid: 1, uname: "", aname: "" }), "authentication not required");
    assert!(matches!(
        t.rpc(&a, Body::Tversion { msize: 100, version: "9P2000.L" }),
        Body::Rversion { msize: 100, version: "9P2000" }
    ));
    assert!(matches!(
        t.rpc(&a, Body::Tversion { msize: 100, version: "2000" }),
        Body::Rversion { version: "unknown", .. }
    ));
    assert_eq!(t.rpc(&a, Body::Tflush { oldtag: 1 }), Body::Rflush);
    // No room for any reply at all.
    assert_eq!(t.server.answer_in_place(&a, &mut [0u8; 4]), None);
    assert_eq!(t.server.answer_in_place(&a, &mut []), None);
    // A reply that does not fit the lend becomes an error that does.
    t.attach(&a, 0, "");
    let reply = t.rpc_with(&a, 40, Body::Tstat { fid: 0 });
    assert_eq!(reply, Body::Rerror { ename: "reply too large" });
}

#[test]
fn random_requests_never_panic() {
    let mut t = T::new();
    let a = alice();
    t.attach(&a, 0, "");
    let mut x = 0x0123_4567_89ab_cdef_u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    // Valid messages with random fields, then random bytes.
    for _ in 0..20_000 {
        let r = next();
        let names = ["a", "..", "b", "notes", "vault", "key", "", "."];
        let wnames: Vec<&str> = (0..r % 4).map(|i| names[((r >> (8 + i * 3)) % 8) as usize]).collect();
        let fid = (r >> 40) as u32 % 6;
        let body = match r % 9 {
            0 => Body::Twalk { fid, newfid: (r >> 50) as u32 % 6, wnames: Names::new(&wnames).unwrap() },
            1 => Body::Topen { fid, mode: (r >> 20) as u8 },
            2 => Body::Tread { fid, offset: next() % 64, count: next() as u32 },
            3 => Body::Twrite { fid, offset: next() % 64, data: b"data" },
            4 => Body::Tclunk { fid },
            5 => Body::Tstat { fid },
            6 => Body::Tattach { fid, afid: NOFID, uname: "", aname: if r & 1 == 0 { "" } else { "a" } },
            7 => Body::Tcreate { fid, name: "c", perm: (r >> 32) as u32, mode: (r >> 24) as u8 },
            _ => Body::Tremove { fid },
        };
        let who = caller(r >> 62, 1000 + (r >> 61), if r & (1 << 33) == 0 { &[] } else { &[7] });
        let _ = t.rpc_with(&who, 512 + (r >> 16) as usize % 4096, body);
    }
    for len in 0..4000 {
        let mut buf: Vec<u8> = (0..len % 300).map(|_| next() as u8).collect();
        if buf.len() >= 7 && len % 2 == 0 {
            let size = (buf.len() as u32).to_le_bytes();
            buf[..4].copy_from_slice(&size);
            buf[4] = 100 + (next() % 28) as u8;
        }
        let _ = t.server.answer_in_place(&a, &mut buf);
    }
}

#[test]
fn every_write_needs_equal_labels() {
    // Answer 51: no blind write-up. A caller with more labels than the object may read it but
    // write nothing into it.
    let mut t = T::new();
    let both = caller(3, 1001, &[7, 8]);
    t.attach(&both, 0, "");
    t.walk(&both, 0, 1, &["vault", "key"]);
    t.open(&both, 1, mode::OREAD).unwrap();
    for mode in [mode::OWRITE, mode::ORDWR, mode::OREAD | mode::OTRUNC] {
        t.walk(&both, 0, 2, &["vault", "key"]);
        assert_eq!(t.open(&both, 2, mode), Err("permission denied".into()), "{mode:#x}");
        t.clunk(&both, 2);
    }
    t.walk(&both, 0, 2, &["vault"]);
    let create = Body::Tcreate { fid: 2, name: "n", perm: 0o644, mode: mode::OWRITE };
    assert_eq!(t.err(&both, create), "permission denied");
    t.walk(&both, 0, 3, &["vault", "key"]);
    assert_eq!(t.err(&both, Body::Tremove { fid: 3 }), "permission denied");
    assert_eq!(t.server.fs.nodes[6].data, b"secret");
}

#[path = "ninep_common_tests.rs"]
mod common;
