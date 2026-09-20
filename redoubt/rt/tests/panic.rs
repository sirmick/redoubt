//! The panic report: with `/dev/cons` in the startup block, a panic's message reaches the
//! console over 9P (the `#[panic_handler]` itself exists only on the machine; it calls
//! `report_panic` and then exits with `exit::PANIC`). Its own test binary, because the runtime
//! reports only the first panic of a process.

mod common;

use std::sync::{Arc, Mutex};

use common::fake;
use redoubt_rt::abi::FOREVER;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event};
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{FileServer, FileStat, NineError, NineServer, Qid, Read};
use redoubt_rt::startup::{Startup, StartupBuilder};

/// A console: the connection's root is the one file; what is written is kept.
struct Console(Arc<Mutex<Vec<u8>>>);

impl FileServer for Console {
    type Node = ();

    fn attach(&mut self, _: &Caller, _: &str) -> Result<((), Qid), NineError> { Ok(((), Qid::default())) }

    fn labels(&self, _: &()) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, _: &(), _: &str) -> Result<((), Qid), NineError> {
        Err(NineError::NOT_FOUND)
    }

    fn open(&mut self, _: &Caller, _: &(), _: u8) -> Result<Qid, NineError> { Ok(Qid::default()) }

    fn read(&mut self, _: &Caller, _: &(), _: u64, _: &mut [u8]) -> Result<Read, NineError> {
        Ok(Read::Done(0))
    }

    fn write(&mut self, _: &Caller, _: &(), _: u64, data: &[u8]) -> Result<usize, NineError> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }

    fn stat(&mut self, _: &Caller, _: &()) -> Result<FileStat, NineError> { Err(NineError::NOT_SUPPORTED) }

    fn dir_entry(&mut self, _: &Caller, _: &(), _: u64) -> Result<Option<((), FileStat)>, NineError> {
        Ok(None)
    }
}

#[test]
fn a_panic_is_reported_on_the_console_once() {
    let f = fake();
    let consoled = f.process(0, &[]);
    let receive = f.endpoint(consoled);
    let program = f.process(7, &[]);
    let cons = f.grant(consoled, receive, program, 1);
    let written = Arc::new(Mutex::new(Vec::new()));

    let console = Console(written.clone());
    let server = f.run(consoled, move || {
        let ep = Endpoint::from_handle(receive);
        let mut server =
            NineServer::new(console, Limits { buckets: 4, in_flight: 0, files: 4, state: 0 }, 0).unwrap();
        while let Ok(Event::Call(request)) = ep.receive(FOREVER, 0) {
            server.serve(request).unwrap();
        }
        0
    });

    let block = StartupBuilder::new(cons.index()).namespace("/dev/cons", cons).finish().unwrap();
    let code = f.run(program, move || {
        let startup = Startup::parse(&block).unwrap();
        redoubt_rt::start::note_console(&startup);
        redoubt_rt::start::report_panic(format_args!("boom at {}", 42));
        // A second panic (say, while reporting the first) prints nothing more.
        redoubt_rt::start::report_panic(format_args!("again"));
        redoubt_rt::exit::PANIC
    });
    assert_eq!(code.join().unwrap(), redoubt_rt::exit::PANIC);
    assert_eq!(String::from_utf8(written.lock().unwrap().clone()).unwrap(), "panicked: boom at 42\n");
    f.destroy(consoled, receive);
    assert_eq!(server.join().unwrap(), 0);
}
