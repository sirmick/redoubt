//! What the client library's tests share: the real `bootfsd`, filled by `init`'s typed calls, and
//! a small in-memory file server for what `bootfsd` does not do (directories, create, remove, a
//! byte quota, littlefsd's typed operations), all on the runtime's fake kernel.

#![allow(dead_code)]

use redoubt_client::file::Connection;
use redoubt_client::{Lend, typed};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event, Request};
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{DMDIR, FileServer, FileStat, NineError, NineServer, QTDIR, Qid, Read};
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::proto::bootfs::{self, Add, Seal};

#[path = "../../../../servers/bootfsd/src/bin/bootfsd.rs"]
mod bootfsd;

/// The entries `init` puts in `/boot`, all public.
pub const BOOT: [(&str, &[u8]); 3] =
    [("keyd", b"\x7fELF keyd"), ("beamlet", b"the vm"), ("iex.beam", b"FOR1")];

/// A running `bootfsd`, filled and sealed by `init` over its typed protocol.
pub struct Boot {
    pub server: usize,
    pub receive: Handle,
    pub init: usize,
    /// `init`'s own connection, badge 1.
    pub founding: Handle,
    thread: std::thread::JoinHandle<u32>,
}

impl Boot {
    pub fn start() -> Boot {
        let f = fake();
        let server = f.process(0, &[]);
        let receive = f.endpoint(server);
        let init = f.process(0, &[]);
        let founding = f.grant(server, receive, init, 1);
        let mut block = StartupBuilder::new(receive.index());
        block.handle("bootfsd", receive).arg("buckets=4");
        for (name, data) in BOOT {
            block.arg(&format!("{}:{name}", data.len()));
        }
        let block = block.finish().unwrap();
        let thread = f.run(server, move || bootfsd::serve(&Startup::parse(&block).unwrap()));
        f.as_process(init, || {
            let to = Endpoint::from_handle(founding);
            let mut lend = Lend::new(1).unwrap();
            for (name, data) in BOOT {
                let add = bootfs::Message::Add(Add { name, offset: 0, data });
                typed::call::<bootfs::Protocol, _>(&to, &mut lend, &add, &[], |_, _| ()).unwrap();
            }
            let seal = bootfs::Message::Seal(Seal {});
            typed::call::<bootfs::Protocol, _>(&to, &mut lend, &seal, &[], |_, _| ()).unwrap();
        });
        Boot { server, receive, init, founding, thread }
    }

    /// A process of `account` holding a fresh connection minted through `init`'s, as a launcher
    /// gives its child.
    pub fn session(&self, account: u64) -> (usize, Handle) {
        let f = fake();
        let (conn, _) = f.as_process(self.init, || {
            let mut lend = Lend::new(1).unwrap();
            Connection::attach(Endpoint::from_handle(self.founding), &mut lend)
                .unwrap()
                .new_connection(&mut lend, "", 0)
                .unwrap()
        });
        let session = f.process(account, &[]);
        let conn = f.copy(self.init, conn.handle(), session);
        (session, conn)
    }

    pub fn stop(self) {
        fake().destroy(self.server, self.receive);
        assert_eq!(self.thread.join().unwrap(), redoubt_rt::exit::OK);
    }
}

#[path = "../../../../servers/keyd/src/bin/keyd.rs"]
mod keyd_bin;

pub const AUDIT_SEED: &str = "2222222222222222222222222222222222222222222222222222222222222222";
pub const AUDIT_BADGE: u64 = 2;
pub const HOST_BADGE: u64 = 1;

/// A running `keyd` with an audit key and a host key.
pub struct Keyd {
    pub server: usize,
    pub receive: Handle,
    thread: std::thread::JoinHandle<u32>,
}

impl Keyd {
    pub fn start() -> Keyd {
        let f = fake();
        let server = f.process(0, &[]);
        let receive = f.endpoint(server);
        let mut block = StartupBuilder::new(receive.index());
        block.handle("keyd", receive).arg("buckets=16");
        block.arg(&format!("host,ssh_host,{}", "1".repeat(64))).arg(&format!("audit,audit,{AUDIT_SEED}"));
        let block = block.finish().unwrap();
        let thread = f.run(server, move || keyd_bin::serve(&Startup::parse(&block).unwrap()));
        Keyd { server, receive, thread }
    }

    pub fn client(&self, badge: u64) -> (usize, Handle) {
        let f = fake();
        let client = f.process(1001, &[]);
        (client, f.grant(self.server, self.receive, client, badge))
    }

    pub fn stop(self) {
        fake().destroy(self.server, self.receive);
        assert_eq!(self.thread.join().unwrap(), 0);
    }
}

/// The in-memory file server's quota ceiling: `new_connection` asking for more is refused.
pub const QUOTA: u64 = 4096;

/// One node: a directory or a file, and where it hangs.
struct Entry {
    name: String,
    parent: usize,
    dir: bool,
    data: Vec<u8>,
    gone: bool,
}

/// An in-memory tree, rooted at node 0, with `home/a/note`, `home/b/secret` and `cons` in it.
pub struct Files {
    entries: Vec<Entry>,
}

impl Files {
    pub fn new() -> Files {
        let mut files = Files { entries: Vec::new() };
        files.add("/", 0, true, b"");
        let home = files.add("home", 0, true, b"");
        let a = files.add("a", home, true, b"");
        files.add("note", a, false, b"hello");
        let b = files.add("b", home, true, b"");
        files.add("secret", b, false, b"b's alone");
        files.add("cons", 0, false, b"");
        files
    }

    fn add(&mut self, name: &str, parent: usize, dir: bool, data: &[u8]) -> usize {
        self.entries.push(Entry { name: name.into(), parent, dir, data: data.to_vec(), gone: false });
        self.entries.len() - 1
    }

    fn qid(&self, node: usize) -> Qid {
        Qid { kind: if self.entries[node].dir { QTDIR } else { 0 }, version: 0, path: node as u64 }
    }

    fn children(&self, dir: usize) -> impl Iterator<Item = usize> + '_ {
        (1..self.entries.len()).filter(move |&i| self.entries[i].parent == dir && !self.entries[i].gone)
    }

    fn stat_of(&self, node: usize) -> FileStat {
        let e = &self.entries[node];
        let mode = if e.dir { DMDIR | 0o755 } else { 0o644 };
        FileStat { qid: self.qid(node), mode, mtime: 0, length: e.data.len() as u64, name: e.name.clone() }
    }

    fn live(&self, node: usize) -> Result<&Entry, NineError> {
        self.entries.get(node).filter(|e| !e.gone).ok_or(NineError::NOT_FOUND)
    }
}

impl FileServer for Files {
    type Node = usize;

    fn attach(&mut self, _: &Caller, _: &str) -> Result<(usize, Qid), NineError> { Ok((0, self.qid(0))) }

    fn minted(&mut self, _: &Caller, _: u64, _: u64, _: &usize, quota: u64) -> Result<(), NineError> {
        if quota > QUOTA { Err(NineError::PERMISSION) } else { Ok(()) }
    }

    fn labels(&self, _: &usize) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, dir: &usize, name: &str) -> Result<(usize, Qid), NineError> {
        let found =
            self.children(*dir).find(|&i| self.entries[i].name == name).ok_or(NineError::NOT_FOUND)?;
        Ok((found, self.qid(found)))
    }

    fn open(&mut self, _: &Caller, node: &usize, _: u8) -> Result<Qid, NineError> {
        self.live(*node)?;
        Ok(self.qid(*node))
    }

    fn read(&mut self, _: &Caller, node: &usize, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        let data = &self.live(*node)?.data;
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(data.len());
        let n = out.len().min(data.len() - start);
        out[..n].copy_from_slice(&data[start..start + n]);
        Ok(Read::Done(n))
    }

    fn write(&mut self, _: &Caller, node: &usize, offset: u64, data: &[u8]) -> Result<usize, NineError> {
        self.live(*node)?;
        let start = usize::try_from(offset).map_err(|_| NineError::BAD_OFFSET)?;
        let file = &mut self.entries[*node].data;
        if file.len() < start + data.len() {
            file.resize(start + data.len(), 0);
        }
        file[start..start + data.len()].copy_from_slice(data);
        Ok(data.len())
    }

    fn stat(&mut self, _: &Caller, node: &usize) -> Result<FileStat, NineError> {
        self.live(*node)?;
        Ok(self.stat_of(*node))
    }

    fn dir_entry(
        &mut self,
        _: &Caller,
        dir: &usize,
        index: u64,
    ) -> Result<Option<(usize, FileStat)>, NineError> {
        Ok(self.children(*dir).nth(index as usize).map(|i| (i, self.stat_of(i))))
    }

    fn create(
        &mut self,
        _: &Caller,
        dir: &usize,
        name: &str,
        perm: u32,
        _: u8,
    ) -> Result<(usize, Qid), NineError> {
        if self.children(*dir).any(|i| self.entries[i].name == name) {
            return Err(NineError::PERMISSION);
        }
        let node = self.add(name, *dir, perm & DMDIR != 0, b"");
        Ok((node, self.qid(node)))
    }

    fn remove(&mut self, _: &Caller, node: &usize) -> Result<(), NineError> {
        if *node == 0 || self.children(*node).next().is_some() {
            return Err(NineError::PERMISSION);
        }
        self.entries[*node].gone = true;
        Ok(())
    }
}

/// A running in-memory file server whose typed opcodes go to `own`.
pub struct Served {
    pub server: usize,
    pub receive: Handle,
    thread: std::thread::JoinHandle<u32>,
}

impl Served {
    pub fn start(
        own: impl FnMut(&mut NineServer<Files>, Request) -> Result<(), Error> + Send + 'static,
    ) -> Served {
        let f = fake();
        let server = f.process(0, &[]);
        let receive = f.endpoint(server);
        let thread = f.run(server, move || {
            // Room in one client's share for every fid a connection can hold.
            let limits = Limits { buckets: 8, in_flight: 0, files: 128, state: 8, requests: 0, pages: 0 };
            let mut nine = NineServer::new(Files::new(), limits, 0x5eed).unwrap();
            let endpoint = Endpoint::from_handle(receive);
            let mut own = own;
            loop {
                match endpoint.receive(FOREVER, 0) {
                    Ok(Event::Call(request)) => {
                        let _ = nine.serve_with(request, &mut own);
                    }
                    Ok(_) => {}
                    Err(_) => return 0,
                }
            }
        });
        Served { server, receive, thread }
    }

    /// A client process of `account` holding a connection of its own, with `badge`.
    pub fn client(&self, account: u64, badge: u64) -> (usize, Handle) {
        let f = fake();
        let client = f.process(account, &[]);
        (client, f.grant(self.server, self.receive, client, badge))
    }

    pub fn stop(self) {
        fake().destroy(self.server, self.receive);
        assert_eq!(self.thread.join().unwrap(), 0);
    }
}
