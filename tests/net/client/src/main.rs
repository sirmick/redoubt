//! `net-client`: one unprivileged program of the D3 rig, in the role its arguments name
//! (`redoubt_net_client::Role`). It talks to `ipd` only through the connection the rig granted it
//! (`net`), exactly as a principal's program would, and to the rig only to report
//! (`redoubt_net_client::REPORT`).

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;

use redoubt_ipd::scope::{Ports, Prefix, Rule, Scope};
use redoubt_net_client::{Args, NET, REPORT, RIG, Role, code, event};
use redoubt_rt::abi::FOREVER;
use redoubt_rt::client::{Client, ClientError};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Buffer;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::proto::{ipd, net_ctl};

redoubt_rt::entry!(run);

/// The connection's root fid, and the scratch fid `clone` is read through.
const ROOT: u32 = 0;
const CLONE: u32 = 1;
/// How many times a waiting read that timed out (`ipd`'s deadline) is asked again.
const WAITS: u32 = 64;

/// Status words of a `ctl` read (NAMESPACES.md, `/net`). A listener's read answers `LISTENING`
/// with the accepted connection's number.
const ESTABLISHED: u32 = 2;
const LISTENING: u32 = 5;

fn run(startup: &Startup) -> u32 {
    let Some(args) = Args::parse(startup.args()) else { return code::BAD_ARGS };
    let rig = startup.handle(RIG).map(Endpoint::from_handle);
    let Some(net) = startup.handle(NET) else { return code::NO_NET };
    let Ok(client) = Client::new(Endpoint::from_handle(net), 2) else { return code::NO_MEMORY };
    let mut me = Me { c: client, net, rig, next_fid: 10 };
    let done = match args.role {
        Role::Echo => me.echo(&args),
        Role::Listen => me.listen(&args),
        Role::Connect => me.connect_once(&args),
        Role::Labelled => me.labelled(),
        Role::Hold => me.hold(),
        Role::Pin => me.pin(&args),
    };
    match done {
        Ok(()) => code::OK,
        Err(code) => code,
    }
}

struct Me {
    c: Client,
    /// The same connection as `c`'s, for a typed call beside 9P.
    net: redoubt_rt::abi::Handle,
    rig: Option<Endpoint>,
    next_fid: u32,
}

/// One socket: its number and the fids of its `ctl` and `data`.
struct Socket {
    ctl: u32,
    data: u32,
}

fn op(message: net_ctl::Message<'_>) -> ([u8; 16], usize) {
    let mut out = [0u8; 16];
    let n = message.encode_file(&mut out).unwrap_or(0);
    (out, n)
}

impl Me {
    fn report(&self, what: u64, value: u64) -> Result<(), u32> {
        let rig = self.rig.as_ref().ok_or(code::REPORT)?;
        rig.call(&[REPORT, what, value, 0], &[], None, FOREVER)
            .into_result()
            .map(|_| ())
            .map_err(|_| code::REPORT)
    }

    fn fid(&mut self) -> u32 {
        self.next_fid += 1;
        self.next_fid
    }

    /// Opens socket `n`'s `ctl` and `data`.
    fn open(&mut self, n: u32) -> Result<Socket, u32> {
        let (ctl, data) = (self.fid(), self.fid());
        self.c.walk(ROOT, ctl, &format!("tcp/{n}/ctl")).map_err(|_| code::OPEN)?;
        self.c.open(ctl, mode::ORDWR).map_err(|_| code::OPEN)?;
        self.c.walk(ROOT, data, &format!("tcp/{n}/data")).map_err(|_| code::OPEN)?;
        self.c.open(data, mode::ORDWR).map_err(|_| code::OPEN)?;
        Ok(Socket { ctl, data })
    }

    /// A new socket: `clone` read once.
    fn socket(&mut self) -> Result<Socket, u32> {
        self.c.walk(ROOT, CLONE, "tcp/clone").map_err(|_| code::CLONE)?;
        self.c.open(CLONE, mode::OREAD).map_err(|_| code::CLONE)?;
        let mut n = [0u8; 4];
        let read = self.c.read(CLONE, 0, &mut n);
        let _ = self.c.clunk(CLONE);
        if read != Ok(4) {
            return Err(code::CLONE);
        }
        self.open(u32::from_le_bytes(n))
    }

    fn ctl(&mut self, s: &Socket, message: net_ctl::Message<'_>, failed: u32) -> Result<(), u32> {
        let (bytes, n) = op(message);
        self.c.write(s.ctl, 0, &bytes[..n]).map(|_| ()).map_err(|_| failed)
    }

    /// A `ctl` read: (state, number). It waits in `ipd`; a wait that ran out is asked again.
    fn status(&mut self, s: &Socket) -> Result<(u32, u32), u32> {
        for _ in 0..WAITS {
            let mut words = [0u8; 8];
            match self.c.read(s.ctl, 0, &mut words) {
                Ok(8) => {
                    let word =
                        |i: usize| u32::from_le_bytes([words[i], words[i + 1], words[i + 2], words[i + 3]]);
                    return Ok((word(0), word(4)));
                }
                Err(ClientError::Remote) => continue,
                _ => return Err(code::STATUS),
            }
        }
        Err(code::STATUS)
    }

    /// Reads exactly `out.len()` bytes of `data`, each read waiting in `ipd`.
    fn read_exact(&mut self, s: &Socket, out: &mut [u8]) -> Result<(), u32> {
        let (mut got, mut waits) = (0, 0);
        while got < out.len() {
            match self.c.read(s.data, 0, &mut out[got..]) {
                Ok(0) => return Err(code::READ),
                Ok(n) => got += n,
                Err(ClientError::Remote) if waits < WAITS => waits += 1,
                Err(_) => return Err(code::READ),
            }
        }
        Ok(())
    }

    fn write_all(&mut self, s: &Socket, mut bytes: &[u8]) -> Result<(), u32> {
        while !bytes.is_empty() {
            let n = self.c.write(s.data, 0, bytes).map_err(|_| code::WRITE)?;
            if n == 0 {
                return Err(code::WRITE);
            }
            bytes = &bytes[n..];
        }
        Ok(())
    }

    fn connect(&mut self, s: &Socket, args: &Args) -> Result<(), u32> {
        let connect = net_ctl::Message::Connect(net_ctl::Connect { addr: &args.addr, port: args.port });
        self.ctl(s, connect, code::REFUSED)
    }

    fn echo(&mut self, args: &Args) -> Result<(), u32> {
        self.c.attach(ROOT, "").map_err(|_| code::ATTACH)?;
        for round in 0..args.times {
            let s = self.socket()?;
            self.connect(&s, args)?;
            if self.status(&s)?.0 != ESTABLISHED {
                return Err(code::STATUS);
            }
            let sent = format!("d3 round trip {round} to port {}\n", args.port);
            self.write_all(&s, sent.as_bytes())?;
            let mut back = [0u8; 64];
            let back = &mut back[..sent.len()];
            self.read_exact(&s, back)?;
            if back != sent.as_bytes() {
                return Err(code::ECHO);
            }
            self.ctl(&s, net_ctl::Message::Close(net_ctl::Close {}), code::CLOSE)?;
        }
        Ok(())
    }

    /// Listens and serves for ever: each accepted connection gets back what it sends first, then
    /// is closed, then reported.
    fn listen(&mut self, args: &Args) -> Result<(), u32> {
        self.c.attach(ROOT, "").map_err(|_| code::ATTACH)?;
        let listener = self.socket()?;
        let listen = net_ctl::Message::Listen(net_ctl::Listen { port: args.port, backlog: args.backlog });
        self.ctl(&listener, listen, code::LISTEN)?;
        self.report(event::READY, 0)?;
        let mut accepted = 0;
        loop {
            let (state, n) = self.status(&listener)?;
            if state != LISTENING {
                return Err(code::STATUS);
            }
            let s = self.open(n)?;
            // One line, echoed as it comes: whatever the segments, the sender gets its line back.
            let (mut buf, mut waits, mut line_done) = ([0u8; 256], 0, false);
            while !line_done {
                let got = match self.c.read(s.data, 0, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(ClientError::Remote) if waits < WAITS => {
                        waits += 1;
                        continue;
                    }
                    Err(_) => return Err(code::READ),
                };
                self.write_all(&s, &buf[..got])?;
                line_done = buf[..got].contains(&b'\n');
            }
            self.ctl(&s, net_ctl::Message::Close(net_ctl::Close {}), code::CLOSE)?;
            accepted += 1;
            self.report(event::ACCEPTED, accepted)?;
        }
    }

    /// One connect: refused (`REFUSED`), or the state it reached (`CONNECTED + state`), after
    /// which the socket is aborted.
    fn connect_once(&mut self, args: &Args) -> Result<(), u32> {
        self.c.attach(ROOT, "").map_err(|_| code::ATTACH)?;
        let s = self.socket()?;
        self.connect(&s, args)?;
        // One wait: if nobody answers, it ends with `ipd`'s `ctl` deadline (or smoltcp's own).
        let mut words = [0u8; 8];
        let state = match self.c.read(s.ctl, 0, &mut words) {
            Ok(8) => u32::from_le_bytes([words[0], words[1], words[2], words[3]]),
            Err(ClientError::Remote) => return Err(code::TIMED_OUT),
            _ => return Err(code::STATUS),
        };
        let _ = self.ctl(&s, net_ctl::Message::Abort(net_ctl::Abort {}), code::CLOSE);
        Err(code::CONNECTED + state.min(5))
    }

    /// Pins `ipd` with abandoned calls (plan 6.5, `d3-net-pinned`): `times` data reads on a quiet
    /// socket, each parked and then given up by this client's short timeout, so `ipd` must free
    /// every one (a leaked one fills this share's parked calls and the next read is refused at
    /// once); then a read with no timeout of its own, which `ipd`'s 30 s deadline must end; then
    /// the echo still works.
    fn pin(&mut self, args: &Args) -> Result<(), u32> {
        self.c.attach(ROOT, "").map_err(|_| code::ATTACH)?;
        let s = self.socket()?;
        self.connect(&s, args)?;
        if self.status(&s)?.0 != ESTABLISHED {
            return Err(code::STATUS);
        }
        let mut buf = [0u8; 64];
        self.c.timeout = 20_000;
        for _ in 0..args.times {
            match self.c.read(s.data, 0, &mut buf) {
                Err(ClientError::Sys(redoubt_rt::abi::Error::Timeout)) => {
                    // The abandoned call keeps the lend: a new client, on the same connection
                    // (whose fids are ipd's), lends a fresh one.
                    self.c = Client::new(Endpoint::from_handle(self.net), 2).map_err(|_| code::NO_MEMORY)?;
                    self.c.timeout = 20_000;
                }
                Err(ClientError::Remote) => return Err(code::PIN_REFUSED),
                _ => return Err(code::READ),
            }
        }
        self.c.timeout = FOREVER;
        if self.c.read(s.data, 0, &mut buf) != Err(ClientError::Remote) {
            return Err(code::NO_DEADLINE);
        }
        let sent = b"d3 still echoing after the pins\n";
        self.write_all(&s, sent)?;
        let mut back = [0u8; 32];
        self.read_exact(&s, &mut back)?;
        if &back[..] != &sent[..] {
            return Err(code::ECHO);
        }
        Ok(())
    }

    /// Every door a labelled caller might try. Each one not refused sets a bit of the report;
    /// then it stays, so a slot it was wrongly given is still held when the rig counts buckets.
    fn labelled(&mut self) -> Result<(), u32> {
        let mut opened = 0;
        let mut tried = |n: u32, ok: bool| {
            if ok {
                opened |= 1 << n;
            }
        };
        tried(0, self.c.attach(ROOT, "").is_ok());
        tried(1, self.c.walk(ROOT, CLONE, "tcp/clone").is_ok());
        tried(2, self.c.open(CLONE, mode::OREAD).is_ok());
        let mut n = [0u8; 4];
        let cloned = self.c.read(CLONE, 0, &mut n) == Ok(4);
        tried(3, cloned);
        // A real connect, on the socket's own `ctl` (socket 0 if clone gave nothing), so that if
        // ipd let a labelled caller through, the SYN would reach its peer, 10.0.9.112:7, and the
        // bench's count of 0 would catch it.
        let number = if cloned { u32::from_le_bytes(n) } else { 0 };
        let ctl = self.fid();
        let opened_ctl = self.c.walk(ROOT, ctl, &format!("tcp/{number}/ctl")).is_ok()
            && self.c.open(ctl, mode::ORDWR).is_ok();
        let wide = Args { role: Role::Connect, addr: [10, 0, 9, 112], port: 7, backlog: 1, times: 1 };
        let connected = self.connect(&Socket { ctl, data: ctl }, &wide).is_ok();
        tried(4, opened_ctl && connected);
        if connected {
            // Wait for it to finish, so the connection is made (and counted) before the rig goes on.
            let _ = self.status(&Socket { ctl, data: ctl });
        }
        tried(5, self.c.new_connection("", 0).is_ok());
        tried(6, self.grant_anything());
        self.report(event::LABELLED, opened)?;
        loop {
            let _ = redoubt_rt::handle::sleep(FOREVER);
        }
    }

    /// `grant` of the widest scope on the connection: whether it was granted.
    fn grant_anything(&mut self) -> bool {
        let any = Rule::Connect(Prefix::new(0, 0).unwrap(), Ports::new(1, 65535).unwrap());
        let Ok(scope) = Scope::new(&[any]) else { return true };
        let bytes = scope.encode();
        let Ok(mut page) = Buffer::new(1) else { return true };
        let Ok(words) = ipd::Message::Grant(ipd::Grant { scope: &bytes }).encode(&mut page) else {
            return true;
        };
        let endpoint = Endpoint::from_handle(self.net);
        match endpoint.call(&words, &[], Some(page), FOREVER).into_result() {
            Ok((reply, page)) => {
                let lend = page.as_deref().unwrap_or(&[]);
                let handles = reply.handles.as_slice().len();
                matches!(ipd::Reply::decode(16, &reply.words, lend, handles), Ok(Ok(_)))
            }
            Err(_) => false,
        }
    }

    fn hold(&mut self) -> Result<(), u32> {
        let attached = self.c.attach(ROOT, "").is_ok();
        self.report(event::ATTACH, u64::from(!attached))?;
        if !attached {
            return Err(code::ATTACH);
        }
        loop {
            let _ = redoubt_rt::handle::sleep(FOREVER);
        }
    }
}
