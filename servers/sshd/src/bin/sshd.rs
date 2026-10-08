//! `sshd`, the program: the core on the box's platform (servers/sshd.md, "Sessions over SSH").
//!
//! - **Listening.** The main thread holds the `ipd` scope `init` hands it, which may listen on TCP port 22
//!   and nothing else. It listens there and hands each accepted connection to an idle slot, or closes it when
//!   every slot is busy.
//! - **A slot** is two threads started once, so no stack is spent per connection (a thread's stack is never
//!   freed): the **driver** owns one connection's core, its channel's `/dev/cons` (the 9P skeleton on the
//!   slot's own endpoint) and every call it makes, to `keyd`, the steward and `ipd`'s writes; the **reader**
//!   only waits in `ipd` for the connection's bytes and hands each read to the driver as a call, answered
//!   once the core has taken it, so a client that sends faster than its session reads is held in TCP, not in
//!   this server. Nothing is shared between threads but handles; a call to `keyd` or the steward holds up
//!   only its own connection.
//! - **Login.** The core asks the platform once a key's signature has verified: the driver mints the
//!   channel's console, a connection to its own `/dev/cons`, and sends it with the login; the steward
//!   launches the session with it at `/dev/cons` and answers its id and labels, which the file then carries
//!   (R25, R67).
//! - **The end.** The steward's `ended` on the console ends the session: its VM is gone or the steward ended
//!   it. A channel the client closes is `channel_closed` to the steward.
//!
//! Every line it writes goes to its own console, `consoled`'s connection `init` gave it.

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

#[cfg(target_os = "none")]
mod machine {
    use alloc::boxed::Box;
    use alloc::format;
    use alloc::rc::Rc;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::cell::RefCell;
    use core::num::NonZeroU64;
    use core::sync::atomic::{AtomicBool, Ordering};

    use redoubt_client::file::Connection as File9;
    use redoubt_client::typed;
    use redoubt_rt::abi::{Error, FOREVER, Handle, Handles, Labels, MAX_LEND_PAGES};
    use redoubt_rt::client::{Connection as Nine, Lend};
    use redoubt_rt::handle::{Endpoint, close};
    use redoubt_rt::ipc::{Caller, Delivery, Event, Request};
    use redoubt_rt::server::close_delivery;
    use redoubt_rt::server::minted::Minter;
    use redoubt_rt::server::ninep::{NineError, NineServer, WORDS_9P, mode, refuse, refuse_malformed};
    use redoubt_rt::server::parked::{NotParked, Parked};
    use redoubt_rt::server::typed::{Outcome, finish};
    use redoubt_rt::startup::Startup;
    use redoubt_rt::wire::proto::{consol, keyd, net_ctl, steward};
    use redoubt_sshd::console::{Chan, Cons, Console, File, LIMITS, Shared, qid};
    use redoubt_sshd::listener::{Again, again, status};
    use redoubt_sshd::slot::{DATA, EOF, Read, Reader};
    use redoubt_sshd::{
        Connection, ExchangeTranscript, Login, Platform, Progress, PublicKey, Refusal, Refused, Signature,
    };

    /// A call's plain answer: status 0, no handles.
    fn done() -> Outcome { Outcome { words: [0; 4], send: Handles::new(), close: Handles::new() } }

    /// The startup block named no `ipd`, `keyd` or `steward`.
    pub const NO_HANDLE: u32 = 2;
    /// `keyd` gave no host key, or `ipd` would not listen, or stopped answering the listener. A
    /// server's own codes start at 4, past the runtime's (`redoubt_rt::exit`).
    pub const NOT_STARTED: u32 = 4;

    /// The port SSH listens on, and how many connections `ipd` may hold for an accept.
    const PORT: u16 = 22;
    const BACKLOG: u8 = 4;
    /// Connections served at once: one parked read each in `ipd`, beside the listener's accept,
    /// within its default of five parked calls a bucket (servers/ipd.md, "Sizing").
    const SLOTS: usize = 4;
    /// The largest packet `sunset` takes, each way.
    const BUF: usize = 35_000;
    /// A driver's stack, in pages: `sunset` moves its exchange state by value a few times
    /// (`redoubt_sshd`'s header). A whole login, client and server in one host thread, needs
    /// more than 88 KiB and fits 96 KiB (x86_64, release); twice that. A slot's stacks have no
    /// guard page between them, so one too small overwrites its neighbour's.
    const DRIVER_STACK_PAGES: usize = 48;
    const READER_STACK_PAGES: usize = 4;
    /// One read from `ipd`, and the lend it travels to the driver in.
    const READ: usize = 4096;

    /// Badges on a slot's endpoint: the main thread's hand-off, and the reader's.
    const MAIN: u64 = 1;
    const READER: u64 = 2;
    /// Word 0 of a call or send on a slot's endpoint that is no 9P (whose word 0 is 0).
    const ACCEPT: u64 = 1;

    /// Fids at `ipd`, which every thread's calls share through the one scope badge: the root,
    /// the main thread's two, and each slot's three from [`slot_fids`].
    const ROOT: u32 = 1;
    const CLONE: u32 = 2;
    const LISTEN_CTL: u32 = 3;
    fn slot_fids(s: usize) -> (u32, u32, u32) {
        let base = 16 + 4 * s as u32;
        (base, base + 1, base + 2)
    }
    /// `ctl` read words (servers/ipd.md, "The `/net` tree").
    const LISTENING: u32 = 5;
    /// How long a refused listen waits before it is asked again (µs).
    const LISTEN_RETRY_US: u64 = 1_000_000;

    /// One of this program's own badges, none of them 0.
    fn badge(b: u64) -> NonZeroU64 { NonZeroU64::new(b).unwrap_or(NonZeroU64::MIN) }

    /// Which slots serve a connection: set by the main thread when it hands one over, cleared by
    /// the slot's driver when the connection is over.
    static BUSY: [AtomicBool; SLOTS] = [const { AtomicBool::new(false) }; SLOTS];

    /// Held while a thread says a line: every thread's lines go through the one console
    /// connection, whose attach resets its fids, so two at once would lose both.
    static SAYING: AtomicBool = AtomicBool::new(false);
    /// How long a thread waits for another's line before asking again (µs).
    const SAY_WAIT_US: u64 = 100;

    /// Says `line` on `sshd`'s own console.
    fn say(console: Option<Handle>, line: &str) {
        let Some(console) = console else { return };
        let Ok(mut lend) = Lend::new(1) else { return };
        while SAYING.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            let _ = redoubt_rt::handle::sleep(SAY_WAIT_US);
        }
        if let Ok(conn) = File9::attach(Endpoint::from_handle(console), &mut lend) {
            if let Ok(file) = conn.open(&mut lend, "", mode::OWRITE) {
                let _ = file.write_at(&mut lend, 0, line.as_bytes());
                let _ = file.close(&mut lend);
            }
        }
        SAYING.store(false, Ordering::Release);
    }

    /// What every thread is given: the handles, by index.
    #[derive(Clone, Copy)]
    struct Handed {
        ipd: Handle,
        keyd: Handle,
        steward: Handle,
        console: Option<Handle>,
        host: PublicKey,
    }

    fn ctl_op(message: net_ctl::Message<'_>) -> ([u8; 16], usize) {
        let mut out = [0u8; 16];
        let n = message.encode_file(&mut out).unwrap_or(0);
        (out, n)
    }

    /// Closes socket `n`'s `ctl`, opened at `ctl`: `ipd` ends the connection and any read waiting on it.
    fn close_socket(ipd: &Nine, lend: &mut Lend, ctl: u32) {
        let (bytes, len) = ctl_op(net_ctl::Message::Close(net_ctl::Close {}));
        let _ = ipd.write(lend, ctl, 0, &bytes[..len]);
    }

    fn open_at(ipd: &Nine, lend: &mut Lend, fid: u32, path: &str) -> bool {
        ipd.walk(lend, ROOT, fid, path).is_ok() && ipd.open(lend, fid, mode::ORDWR).is_ok()
    }

    /// The reader: waits for its slot's socket on `wake`, then hands each read to the driver until
    /// the connection ends, and waits again.
    fn reader(s: usize, ipd: Handle, wake: Handle, driver: Handle) {
        let (wake, driver) = (Endpoint::from_handle(wake), Endpoint::from_handle(driver));
        let ipd = Nine::new(Endpoint::from_handle(ipd));
        let Ok(mut lend) = Lend::new(1) else { return };
        let (_, data, _) = slot_fids(s);
        let mut buf = vec![0u8; READ];
        loop {
            match wake.receive(FOREVER, 0) {
                Ok(Event::Send(d)) => close_delivery(&d),
                Ok(Event::Call(r)) => {
                    drop(r);
                    continue;
                }
                Ok(_) => continue,
                Err(_) => return,
            }
            let mut tries = 0;
            loop {
                let n = match ipd.read(&mut lend, data, 0, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => match again(&e, tries) {
                        Again::Now => continue,
                        Again::After(us) => {
                            tries += 1;
                            let _ = redoubt_rt::handle::sleep(us);
                            continue;
                        }
                        Again::No => break,
                    },
                };
                tries = 0;
                let Ok(pages) = lend.pages() else { break };
                pages[..n].copy_from_slice(&buf[..n]);
                if !lend.call(&driver, &[DATA, n as u64, 0, 0], &[], FOREVER).status.is_ok() {
                    break;
                }
            }
            let _ = driver.call(&[EOF, 0, 0, 0], &[], None, FOREVER);
        }
    }

    /// Mints the console on the slot's own endpoint.
    struct Own<'e>(&'e Endpoint);

    impl Minter for Own<'_> {
        fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
            Ok(self.0.mint(badge, None)?.handle())
        }

        fn random(&mut self) -> Result<u64, Error> { redoubt_rt::handle::random_u64() }
    }

    /// The box's platform for one connection.
    struct Slot<'e> {
        handed: Handed,
        endpoint: &'e Endpoint,
        lend: Lend,
        nine: NineServer<Cons>,
        parked: Parked<()>,
        /// The console the last login minted, by badge, while its session runs.
        console: Option<u64>,
    }

    /// The driver's own caller, through which it mints the console.
    fn me() -> Caller { Caller { badge: MAIN, account: 0, labels: Labels::new() } }

    impl Slot<'_> {
        fn say(&self, line: &str) { say(self.handed.console, line) }

        fn keyd<T>(
            &mut self,
            m: &keyd::Message<'_>,
            read: impl FnOnce(keyd::Reply<'_>) -> Option<T>,
        ) -> Option<T> {
            let keyd = Endpoint::from_handle(self.handed.keyd);
            typed::call::<keyd::Protocol, _>(&keyd, &mut self.lend, m, &[], |r, _| read(r)).ok().flatten()
        }

        /// Answers `request`, or parks it if the console asked to wait.
        fn serve(&mut self, request: Request, now: u64) {
            let Ok(Some(request)) = self.nine.serve_parking(request, |_, request| refuse_malformed(request))
            else {
                return;
            };
            let charge = self.nine.charge_of(&request.caller);
            if let Err(NotParked(request)) =
                self.parked.park(self.nine.admission_mut(), request, charge, (), now)
            {
                let _ = refuse(request, NineError::TOO_MANY);
            }
        }

        /// Serves every parked call again: input or room may have come.
        fn turn(&mut self, now: u64) {
            for _ in 0..self.parked.len() {
                let Some(call) = self.parked.resume_first(self.nine.admission_mut(), |_| true) else { break };
                let Ok((request, ())) = call else { continue };
                self.serve(request, now);
            }
            self.nine.wake(now);
        }
    }

    impl Platform for Slot<'_> {
        type Session = Console;

        fn sign_exchange(&mut self, t: &ExchangeTranscript<'_>) -> Result<Signature, Refused> {
            let m = keyd::Message::SignSshExchange(keyd::SignSshExchange {
                v_c: t.v_c,
                v_s: t.v_s,
                i_c: t.i_c,
                i_s: t.i_s,
                q_c: t.q_c,
                q_s: t.q_s,
                k: t.k,
            });
            let signed = self.keyd(&m, |r| match r {
                keyd::Reply::SignSshExchange(r) => Signature::try_from(r.signature).ok(),
                _ => None,
            });
            if signed.is_none() {
                self.say("sshd: keyd refused the exchange\n");
            }
            signed.ok_or(Refused)
        }

        fn holds(&mut self, key: &PublicKey) -> Result<bool, Refused> {
            let m = keyd::Message::Holds(keyd::Holds { key });
            let held = self.keyd(&m, |r| match r {
                keyd::Reply::Holds(r) => Some(r.held != 0),
                _ => None,
            });
            match held {
                Some(true) => self.say("sshd: keyd holds the key offered\n"),
                None => self.say("sshd: keyd did not answer whether it holds the key offered\n"),
                Some(false) => {}
            }
            held.ok_or(Refused)
        }

        fn login(&mut self, who: &Login<'_>, key: &PublicKey) -> Result<Console, Refused> {
            let name = match who.label {
                Some(label) => format!("{}+{label}", who.principal),
                None => String::from(who.principal),
            };
            let made = self.nine.mint_rooted(&me(), (File, qid()), &mut Own(self.endpoint));
            let Ok((console, _, badge)) = made else {
                self.say(&format!("sshd: login {name}: no console\n"));
                return Err(Refused);
            };
            let m = steward::Message::Login(steward::Login {
                principal: who.principal,
                label: who.label.unwrap_or(""),
                key,
            });
            let steward = Endpoint::from_handle(self.handed.steward);
            let answer = typed::call::<steward::Protocol, _>(
                &steward,
                &mut self.lend,
                &m,
                &[console],
                |r, _| match r {
                    steward::Reply::Login(l) => {
                        let mut labels: Vec<u64> = redoubt_rt::wire::labels::decode(l.labels).collect();
                        labels.sort_unstable();
                        labels.dedup();
                        Some((l.session, labels))
                    }
                    _ => None,
                },
            );
            // The steward keeps its own copy for the session's `/dev/cons`.
            let _ = close(console);
            match answer {
                Ok(Some((id, labels))) => {
                    self.say(&format!("sshd: login {name}: session {id:016x}, labels {labels:?}\n"));
                    let labelled = !labels.is_empty();
                    self.nine.fs.labels = labels;
                    self.console = Some(badge);
                    Ok(Console::new(self.nine.fs.chan.clone(), id, labelled))
                }
                refused => {
                    let why = match refused {
                        Err(redoubt_client::Error::Server(code)) => steward::ErrorCode::from_code(code)
                            .map_or_else(|| format!("{code}"), |c| format!("{c:?}")),
                        _ => String::from("no answer"),
                    };
                    self.say(&format!("sshd: login {name}: refused ({why})\n"));
                    self.nine.unmint(badge);
                    Err(Refused)
                }
            }
        }

        fn end(&mut self, session: Console) {
            let m = steward::Message::ChannelClosed(steward::ChannelClosed { session: session.id });
            let steward = Endpoint::from_handle(self.handed.steward);
            let _ = typed::call::<steward::Protocol, _>(&steward, &mut self.lend, &m, &[], |_, _| ());
            session.chan.borrow_mut().end(0);
            self.say(&format!("sshd: session {:016x} ended\n", session.id));
        }

        fn refused(&mut self, request: Refusal) {
            self.say(&format!("sshd: request {} refused\n", request.name()))
        }
    }

    /// Whether `d` is `consol`'s `ended`.
    fn ended(d: &Delivery) -> bool {
        matches!(consol::Message::decode(&d.words, &[], 0), Ok(consol::Message::Ended(_)))
    }

    /// One connection, from its accept to its end.
    fn connection(s: usize, sock: u32, handed: Handed, endpoint: &Endpoint, wake: &Endpoint) {
        let ipd = Nine::new(Endpoint::from_handle(handed.ipd));
        let Ok(mut lend) = Lend::new(1) else { return };
        let (ctl, data, out) = slot_fids(s);
        let opened = open_at(&ipd, &mut lend, ctl, &format!("tcp/{sock}/ctl"))
            && open_at(&ipd, &mut lend, data, &format!("tcp/{sock}/data"))
            && open_at(&ipd, &mut lend, out, &format!("tcp/{sock}/data"));
        let started = opened && wake.send(&[ACCEPT, 0, 0, 0], &[], None, FOREVER).is_ok();
        if started {
            let mut reader = drive(handed, endpoint, &ipd, &mut lend, out);
            close_socket(&ipd, &mut lend, ctl);
            // The reader ends with the socket; its last call says so, unless the driver took it
            // already (a client that hung up first): the slot is free once it is taken.
            while reader.waits() {
                match endpoint.receive(FOREVER, MAX_LEND_PAGES) {
                    Ok(Event::Call(r)) if r.caller.badge == READER => {
                        reader.took(r.words[0]);
                        let _ = finish(r, &done());
                    }
                    Ok(Event::Call(r)) => {
                        let _ = refuse(r, NineError::NO_CONNECTION);
                    }
                    Ok(Event::Send(d)) => close_delivery(&d),
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        } else {
            say(handed.console, &format!("sshd: connection {sock} did not start\n"));
            if opened {
                close_socket(&ipd, &mut lend, ctl);
            }
        }
        for fid in [ctl, data, out] {
            let _ = ipd.clunk(&mut lend, fid);
        }
    }

    /// Runs the core over the socket until the connection is over. Returns the reader as the
    /// driver heard it: whether its last call was taken already.
    fn drive(handed: Handed, endpoint: &Endpoint, ipd: &Nine, ipd_lend: &mut Lend, out: u32) -> Reader {
        let mut reader = Reader::default();
        let chan: Shared = Rc::new(RefCell::new(Chan::default()));
        let made = redoubt_rt::handle::random_u64().ok().and_then(|random| {
            let nine = NineServer::new(Cons { chan: chan.clone(), labels: Vec::new() }, LIMITS, random);
            Some((nine.ok()?, Lend::new(2).ok()?))
        });
        let Some((nine, lend)) = made else {
            say(handed.console, "sshd: a connection's console could not be made\n");
            return reader;
        };
        let mut slot = Slot { handed, endpoint, lend, nine, parked: Parked::new(FOREVER), console: None };
        slot.nine.requests_wait(FOREVER);
        let (mut inbuf, mut outbuf) = (vec![0u8; BUF], vec![0u8; BUF]);
        let mut conn = Box::new(Connection::new(&mut inbuf, &mut outbuf, &handed.host));
        // The reader's call whose bytes the core has not all taken: answered once it has.
        let mut held: Option<Request> = None;
        let mut pending: Vec<u8> = Vec::new();
        loop {
            let now = redoubt_rt::handle::time_now().unwrap_or(0);
            // The core, until it is idle: input it takes, output it makes, the session's bytes.
            let closed = loop {
                let n = match conn.input(&pending) {
                    Ok(n) => n,
                    Err(e) => {
                        slot.say(&format!("sshd: a connection failed: {e:?}\n"));
                        break true;
                    }
                };
                pending.drain(..n);
                let progress = match conn.progress(&mut slot) {
                    Ok(p) => p,
                    Err(e) => {
                        slot.say(&format!("sshd: a connection failed: {e:?}\n"));
                        break true;
                    }
                };
                let (mut written, mut tries) = (0, 0);
                let bytes = conn.output_buf();
                while written < bytes.len() {
                    match ipd.write(ipd_lend, out, 0, &bytes[written..]) {
                        Ok(0) => break,
                        Ok(w) => (written, tries) = (written + w, 0),
                        Err(e) => match again(&e, tries) {
                            Again::Now => {}
                            Again::After(us) => {
                                tries += 1;
                                let _ = redoubt_rt::handle::sleep(us);
                            }
                            Again::No => break,
                        },
                    }
                }
                let all = written == bytes.len();
                conn.consume_output(written);
                if !all {
                    slot.say("sshd: a connection's bytes could not be sent\n");
                    break true;
                }
                slot.turn(now);
                match progress {
                    Progress::Busy => continue,
                    Progress::Idle if n > 0 && !pending.is_empty() => continue,
                    Progress::Idle => break false,
                    Progress::Closed => {
                        slot.say("sshd: a connection closed\n");
                        break true;
                    }
                }
            };
            if pending.is_empty() {
                if let Some(r) = held.take() {
                    let _ = finish(r, &done());
                }
            }
            if closed {
                break;
            }
            match endpoint.receive(FOREVER, MAX_LEND_PAGES) {
                Ok(Event::Call(mut r)) if r.caller.badge == READER => match reader.took(r.words[0]) {
                    Read::Data => {
                        let n = (r.words[1] as usize).min(READ);
                        let bytes = r.lend();
                        let n = n.min(bytes.len());
                        if pending.try_reserve(n).is_ok() {
                            pending.extend_from_slice(&bytes[..n]);
                        }
                        held = Some(r);
                    }
                    Read::End => {
                        let _ = finish(r, &done());
                        slot.say("sshd: a connection's input ended\n");
                        conn.close_input();
                    }
                },
                Ok(Event::Call(r)) => slot.serve(r, now),
                // The steward's `ended`, on the console it was given: the session is over.
                Ok(Event::Send(d)) if Some(d.caller.badge) == slot.console && ended(&d) => {
                    close_delivery(&d);
                    chan.borrow_mut().end(0);
                }
                Ok(Event::Send(d)) => {
                    if let Some(other) = slot.nine.deliver(d, now) {
                        close_delivery(&other);
                    }
                }
                Ok(Event::Abandoned(id)) => {
                    if !slot.nine.abandoned(id) {
                        slot.parked.abandoned(slot.nine.admission_mut(), id, &WORDS_9P);
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        // Whatever still waits on the console finds its end.
        chan.borrow_mut().end(0);
        let now = redoubt_rt::handle::time_now().unwrap_or(0);
        slot.turn(now);
        if let Some(r) = held.take() {
            let _ = finish(r, &done());
        }
        reader
    }

    /// A slot's driver: waits for a connection on `endpoint`, serves it, and waits again.
    fn driver(s: usize, handed: Handed, endpoint: Handle, wake: Handle) {
        let (endpoint, wake) = (Endpoint::from_handle(endpoint), Endpoint::from_handle(wake));
        loop {
            match endpoint.receive(FOREVER, MAX_LEND_PAGES) {
                Ok(Event::Send(d)) if d.caller.badge == MAIN && d.words[0] == ACCEPT => {
                    close_delivery(&d);
                    connection(s, d.words[1] as u32, handed, &endpoint, &wake);
                    BUSY[s].store(false, Ordering::Release);
                }
                Ok(Event::Send(d)) => close_delivery(&d),
                Ok(Event::Call(r)) => {
                    let _ = refuse(r, NineError::NO_CONNECTION);
                }
                Ok(_) => {}
                Err(_) => return,
            }
        }
    }

    /// A new socket at `ipd`, listening on [`PORT`] with its `ctl` open at [`LISTEN_CTL`].
    fn listen(net: &Nine, lend: &mut Lend) -> bool {
        let cloned =
            net.walk(lend, ROOT, CLONE, "tcp/clone").is_ok() && net.open(lend, CLONE, mode::OREAD).is_ok();
        let mut n = [0u8; 4];
        let cloned = cloned && net.read(lend, CLONE, 0, &mut n) == Ok(4);
        let _ = net.clunk(lend, CLONE);
        let (bytes, len) = ctl_op(net_ctl::Message::Listen(net_ctl::Listen { port: PORT, backlog: BACKLOG }));
        let opened = cloned && open_at(net, lend, LISTEN_CTL, &format!("tcp/{}/ctl", u32::from_le_bytes(n)));
        if opened && net.write(lend, LISTEN_CTL, 0, &bytes[..len]).is_ok() {
            return true;
        }
        if opened {
            close_socket(net, lend, LISTEN_CTL);
            let _ = net.clunk(lend, LISTEN_CTL);
        }
        false
    }

    /// Listens, and hands each connection to an idle slot.
    pub fn serve(startup: &Startup) -> u32 {
        let (Some(ipd), Some(keyd), Some(steward)) =
            (startup.handle("ipd"), startup.handle("keyd"), startup.handle("steward"))
        else {
            return NO_HANDLE;
        };
        let console = startup.namespace().find(|(path, _)| *path == "/dev/cons").map(|(_, h)| h);
        let Ok(mut lend) = Lend::new(1) else { return NOT_STARTED };
        let keyd_ep = Endpoint::from_handle(keyd);
        let public = keyd::Message::PublicKey(keyd::PublicKey {});
        let host = typed::call::<keyd::Protocol, _>(&keyd_ep, &mut lend, &public, &[], |r, _| match r {
            keyd::Reply::PublicKey(r) => PublicKey::try_from(r.key).ok(),
            _ => None,
        });
        let Ok(Some(host)) = host else {
            say(console, "sshd: keyd gave no host key\n");
            return NOT_STARTED;
        };
        let handed = Handed { ipd, keyd, steward, console, host };
        let net = Nine::new(Endpoint::from_handle(ipd));
        if net.attach(&mut lend, ROOT, "").is_err() {
            say(console, "sshd: ipd would not attach\n");
            return NOT_STARTED;
        }
        // `ipd` refuses a listen until it has a link: a box with no network yet waits for one.
        let mut said = false;
        while !listen(&net, &mut lend) {
            if !said {
                say(console, "sshd: ipd would not listen on port 22; asking again each second\n");
                said = true;
            }
            let _ = redoubt_rt::handle::sleep(LISTEN_RETRY_US);
        }
        // Each slot's two threads, started once.
        let mut slots = Vec::new();
        for s in 0..SLOTS {
            let (Ok(endpoint), Ok(wake)) = (Endpoint::create(), Endpoint::create()) else {
                return NOT_STARTED;
            };
            let (Ok(main), Ok(reader_badge), Ok(wake_badge)) = (
                endpoint.mint(badge(MAIN), None),
                endpoint.mint(badge(READER), None),
                wake.mint(badge(MAIN), None),
            ) else {
                return NOT_STARTED;
            };
            let (e, w, r, wb) =
                (endpoint.handle(), wake.handle(), reader_badge.handle(), wake_badge.handle());
            let started =
                redoubt_rt::thread::spawn(Box::new(move || driver(s, handed, e, wb)), DRIVER_STACK_PAGES)
                    .and_then(|_| {
                        redoubt_rt::thread::spawn(Box::new(move || reader(s, ipd, w, r)), READER_STACK_PAGES)
                    });
            if started.is_err() {
                return NOT_STARTED;
            }
            slots.push(main);
        }
        say(console, "sshd: listening on port 22\n");
        loop {
            let Some((state, sock)) = status(&net, &mut lend, LISTEN_CTL) else { return NOT_STARTED };
            if state != LISTENING {
                return NOT_STARTED;
            }
            let idle = (0..SLOTS).find(|&s| {
                BUSY[s].compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok()
            });
            let handed_to =
                idle.filter(|&s| slots[s].send(&[ACCEPT, u64::from(sock), 0, 0], &[], None, FOREVER).is_ok());
            if let Some(s) = handed_to {
                say(console, &format!("sshd: connection {sock} on slot {s}\n"));
            }
            if handed_to.is_none() {
                // Every slot is busy: the connection is closed, and the client told nothing more.
                const SPARE: u32 = 4;
                if open_at(&net, &mut lend, SPARE, &format!("tcp/{sock}/ctl")) {
                    close_socket(&net, &mut lend, SPARE);
                    let _ = net.clunk(&mut lend, SPARE);
                }
                if let Some(s) = idle {
                    BUSY[s].store(false, Ordering::Release);
                }
                say(console, "sshd: every slot is busy: a connection is closed\n");
            }
        }
    }

    /// `getrandom`'s only source on bare metal (vendor/README.md, "sshd's SSH library"): the
    /// kernel's generator, eight bytes a word, every byte of `dest` written before `Ok`.
    ///
    /// # Safety
    ///
    /// `dest` must be valid for writes of `len` bytes, which nothing else reads or writes until
    /// it returns: `getrandom`, its only caller, passes a buffer it owns.
    #[no_mangle]
    unsafe extern "Rust" fn __getrandom_v03_custom(
        dest: *mut u8,
        len: usize,
    ) -> Result<(), getrandom::Error> {
        // SAFETY: `getrandom` passes a buffer of `len` writable bytes it owns for this call, and
        // nothing else reads or writes it until we return.
        let dest = unsafe { core::slice::from_raw_parts_mut(dest, len) };
        for chunk in dest.chunks_mut(8) {
            let word = redoubt_rt::handle::random_u64().map_err(|_| getrandom::Error::UNSUPPORTED)?;
            chunk.copy_from_slice(&word.to_le_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}

redoubt_rt::entry!(machine::serve);
