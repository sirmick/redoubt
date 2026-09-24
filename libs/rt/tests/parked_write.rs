//! A write that waits (`Write::Wait`, answer 174) under `serve_parking`, on the fake kernel: it is
//! parked like a waiting read, charged to the same admission, answered at once when its caller
//! gives up, and refused with the server's timeout when its deadline passes (QA
//! D3-code-review-3).

mod common;

use std::time::{Duration, Instant};

use common::fake;
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::client::{Client, ClientError};
use redoubt_rt::handle::{self, Endpoint};
use redoubt_rt::ipc::{Caller, Event};
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{
    FileServer, FileStat, MALFORMED, NineError, NineServer, Qid, Read, Write, mode, refuse,
};
use redoubt_rt::server::parked::{NotParked, Parked};

/// One file whose every write waits: a socket whose send buffer never empties.
struct Full;

impl FileServer for Full {
    type Node = ();

    fn attach(&mut self, _: &Caller, _: &str) -> Result<((), Qid), NineError> {
        Ok(((), Qid { kind: 0, version: 0, path: 0 }))
    }

    fn labels(&self, _: &()) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, _: &(), _: &str) -> Result<((), Qid), NineError> {
        Err(NineError::NOT_DIR)
    }

    fn open(&mut self, _: &Caller, _: &(), _: u8) -> Result<Qid, NineError> {
        Ok(Qid { kind: 0, version: 0, path: 0 })
    }

    fn read(&mut self, _: &Caller, _: &(), _: u64, _: &mut [u8]) -> Result<Read, NineError> {
        Ok(Read::Done(0))
    }

    fn write(&mut self, _: &Caller, _: &(), _: u64, _: &[u8]) -> Result<usize, NineError> { Ok(0) }

    fn write_or_wait(&mut self, _: &Caller, _: &(), _: u64, _: &[u8]) -> Result<Write, NineError> {
        Ok(Write::Wait)
    }

    fn stat(&mut self, _: &Caller, _: &()) -> Result<FileStat, NineError> { Ok(FileStat::default()) }

    fn dir_entry(&mut self, _: &Caller, _: &(), _: u64) -> Result<Option<((), FileStat)>, NineError> {
        Ok(None)
    }
}

/// How long a parked write waits before the server refuses it.
const LONGEST: u64 = 300_000;

#[test]
fn a_waiting_write_is_parked_abandoned_and_expired() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive = f.endpoint(server);
    let conn = f.grant(server, receive, client, 1);

    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let limits = Limits { buckets: 2, in_flight: 4, files: 4, state: 0 };
        let mut nine = NineServer::new(Full, limits, 5).unwrap();
        let mut parked: Parked<()> = Parked::new(LONGEST);
        let own = |_: &mut NineServer<Full>, r: redoubt_rt::ipc::Request| {
            r.reply(&MALFORMED, &[]).map(|_| ()).map_err(|(e, _)| e)
        };
        let (mut abandoned, mut expired) = (0, 0);
        loop {
            let now = handle::time_now().unwrap();
            while let Some(call) = parked.expired(nine.admission_mut(), now) {
                let (request, ()) = call.unwrap();
                refuse(request, NineError("timeout")).unwrap();
                expired += 1;
            }
            let timeout = parked.next_deadline().map_or(FOREVER, |d| d.saturating_sub(now).max(1));
            match ep.receive(timeout, 0) {
                Ok(Event::Call(request)) => {
                    let Ok(Some(held)) = nine.serve_parking(request, own) else { continue };
                    // Its deadline runs from now, not from before the receive that waited.
                    let now = handle::time_now().unwrap();
                    let share = nine.share_of(&held.caller);
                    if let Err(NotParked(r)) = parked.park(nine.admission_mut(), held, share, (), now) {
                        refuse(r, NineError("too_many")).unwrap();
                    }
                }
                Ok(Event::Abandoned(id)) => {
                    abandoned += u32::from(parked.abandoned(nine.admission_mut(), id, &MALFORMED).is_some());
                }
                Ok(_) | Err(Error::Timeout) => {}
                Err(_) => return abandoned * 10 + expired,
            }
        }
    });

    f.as_process(client, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        c.attach(0, "").unwrap();
        c.open(0, mode::OWRITE).unwrap();
        // Its caller gives up first: the write is answered at once, freeing it.
        c.timeout = 50_000;
        assert_eq!(c.write(0, 0, b"lost"), Err(ClientError::Sys(Error::Timeout)));
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while f.open_calls(server) != 0 {
        assert!(Instant::now() < deadline, "the abandoned write was never answered");
        std::thread::sleep(Duration::from_millis(1));
    }
    f.as_process(client, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        // This one waits it out: the server's deadline answers it, with an Rerror.
        c.timeout = FOREVER;
        let started = Instant::now();
        assert_eq!(c.write(0, 0, b"late"), Err(ClientError::Remote));
        assert!(started.elapsed() >= Duration::from_micros(LONGEST), "answered before its deadline");
    });
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 11, "one abandoned and one expired write");
}
