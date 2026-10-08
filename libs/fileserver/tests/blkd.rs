//! The one client of `blkd`'s range, [`Blkd`], against a fake `blkd` on the runtime's fake kernel,
//! over the real protocol: reads split at what one call carries, and a read of part of a sector
//! refused as a fault, never a panic.

use std::sync::{Arc, Mutex};

use redoubt_fake_kernel::fake;
use redoubt_fileserver::range::{Blkd, Fault, Geometry, Range, SECTOR};
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event, Words};
use redoubt_rt::server::typed::{Answer, Protocol, TypedServer, serve_call};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::blkd::{self, ErrorCode, InfoReply, Message, ReadReply};

const S: usize = SECTOR as usize;

/// `blkd`'s protocol, for the fake.
struct Proto;

impl Protocol for Proto {
    type Error = ErrorCode;
    type Reply<'a> = blkd::Reply<'a>;
    type Request<'a> = Message<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Message<'a>, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &blkd::Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

/// A read-only range in memory behind `blkd`'s protocol, counting the reads it answers.
struct FakeBlkd {
    bytes: Vec<u8>,
    reads: Arc<Mutex<usize>>,
    out: Vec<u8>,
}

impl TypedServer<Proto> for FakeBlkd {
    fn handle<'s>(
        &'s mut self,
        _: &Caller,
        request: Message<'_>,
        _: &[Handle],
    ) -> Result<Answer<blkd::Reply<'s>>, ErrorCode> {
        match request {
            Message::Info(_) => Ok(Answer::new(blkd::Reply::Info(InfoReply {
                sectors: (self.bytes.len() / S) as u64,
                sector_size: SECTOR,
                read_only: 1,
            }))),
            Message::Read(r) => {
                *self.reads.lock().unwrap() += 1;
                let start = r.sector as usize * S;
                let end = start + r.count as usize * S;
                self.out = self.bytes.get(start..end).ok_or(ErrorCode::OutOfRange)?.to_vec();
                Ok(Answer::new(blkd::Reply::Read(ReadReply { data: &self.out })))
            }
            _ => Err(ErrorCode::NotPermitted),
        }
    }
}

/// A read of whole sectors splits at what the lend carries, one call each; a read of part of a
/// sector, which the protocol cannot ask for, is a fault and asks nothing; and a byte read takes
/// the piece out of the sectors around it.
#[test]
fn reads_split_at_the_lend_and_part_of_a_sector_is_a_fault() {
    let f = fake();
    let bytes: Vec<u8> = (0..64 * S).map(|i| (i % 251) as u8).collect();
    let reads = Arc::new(Mutex::new(0));
    let blkd = f.process(0, &[]);
    let receive = f.endpoint(blkd);
    let server = FakeBlkd { bytes: bytes.clone(), reads: reads.clone(), out: Vec::new() };
    let thread = f.run(blkd, move || {
        let (endpoint, mut server) = (Endpoint::from_handle(receive), server);
        loop {
            match endpoint.receive(FOREVER, 0) {
                Ok(Event::Call(request)) => {
                    let _ = serve_call::<Proto, _>(&mut server, request);
                }
                Ok(_) => {}
                Err(_) => return 0,
            }
        }
    });
    let client = f.process(0, &[]);
    let volume = f.grant(blkd, receive, client, 1);
    let calls = |reads: &Arc<Mutex<usize>>| std::mem::take(&mut *reads.lock().unwrap());
    f.as_process(client, || {
        // Two pages: fifteen sectors a call beside the message.
        let mut range = Blkd::new(Endpoint::from_handle(volume), 2).unwrap();
        assert_eq!(range.info(), Ok(Geometry { sectors: 64, read_only: true }));
        let mut out = vec![0u8; 20 * S];
        range.read(3, &mut out).unwrap();
        assert_eq!(out, bytes[3 * S..23 * S]);
        assert_eq!(calls(&reads), 2, "fifteen sectors, then five");
        for len in [1, S - 1, S + 1, 2 * S + 100] {
            let mut out = vec![0u8; len];
            assert_eq!(range.read(0, &mut out), Err(Fault), "{len} bytes");
        }
        assert_eq!(calls(&reads), 0, "a part of a sector asks nothing of the range");
        let mut out = vec![0u8; 50];
        range.read_at(S as u64 - 10, &mut out).unwrap();
        assert_eq!(out, bytes[S - 10..S + 40]);
        assert_eq!(calls(&reads), 1, "the two sectors around it, in one call");
        assert_eq!(range.read(u64::MAX, &mut [0u8; S]), Err(Fault));
        assert_eq!(range.read(63, &mut [0u8; 2 * S]), Err(Fault), "past the range: blkd's refusal");
    });
    f.destroy(blkd, receive);
    assert_eq!(thread.join().unwrap(), 0);
}
