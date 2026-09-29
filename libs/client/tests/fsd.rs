//! `fsd`'s typed operations through the library, against an in-test server on the fake kernel
//! until `fsd` exists: each operation names exactly the fids of the files it was given, its reply
//! and error codes come back decoded, and two files on different connections are refused before
//! any call, since a fid means nothing on another connection.

mod common;

use std::sync::{Arc, Mutex};

use common::Served;
use redoubt_client::file::Connection;
use redoubt_client::{Error, Lend, Refusal, fsd};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::Handles;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::wire::proto::fsd::{
    CopyFileReply, ErrorCode, GetAttrReply, Message, RenameReply, Reply, SetAttrReply,
};

/// What the server saw of one operation: its name and the fids it named.
type Seen = Arc<Mutex<Vec<(&'static str, Vec<u32>)>>>;

/// Answers fsd's operations as a volume would, recording each; attribute 7 is refused.
fn start(seen: Seen) -> Served {
    Served::start(move |_, mut request| {
        let words = request.words;
        let handles = request.handles.as_slice().len();
        let (name, fids, reply) = match Message::decode(&words, request.lend(), handles) {
            Ok(Message::Rename(r)) => {
                ("rename", vec![r.old_dir, r.new_dir], Ok(Reply::Rename(RenameReply {})))
            }
            Ok(Message::CopyFile(c)) => {
                ("copy_file", vec![c.src_fid, c.dst_dir], Ok(Reply::CopyFile(CopyFileReply { count: 42 })))
            }
            Ok(Message::SetAttr(s)) if s.attr == 7 => ("set_attr", vec![s.fid], Err(ErrorCode::Refused)),
            Ok(Message::SetAttr(s)) => ("set_attr", vec![s.fid], Ok(Reply::SetAttr(SetAttrReply {}))),
            Ok(Message::GetAttr(g)) => {
                ("get_attr", vec![g.fid], Ok(Reply::GetAttr(GetAttrReply { value: b"blue" })))
            }
            Err(_) => ("malformed", vec![], Err(ErrorCode::Malformed)),
        };
        seen.lock().unwrap().push((name, fids));
        let words = match reply {
            Ok(reply) => reply.encode(request.lend()).unwrap(),
            Err(code) => code.encode(),
        };
        finish(request, &Outcome { words, send: Handles::new(), close: Handles::new() }).map(|_| ())
    })
}

#[test]
fn each_operation_names_its_files_fids() {
    let seen = Seen::default();
    let served = start(seen.clone());
    let (client, conn) = served.client(1001, 1);
    fake().as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let conn = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        let a = conn.open(&mut lend, "home/a", mode::OREAD).unwrap();
        let b = conn.open(&mut lend, "home/b", mode::OREAD).unwrap();
        let note = conn.open(&mut lend, "home/a/note", mode::OREAD).unwrap();
        fsd::rename(&mut lend, &a, "note", &b, "moved").unwrap();
        assert_eq!(fsd::copy_file(&mut lend, &note, &b, "copy"), Ok(42));
        fsd::set_attr(&mut lend, &note, 16, b"blue").unwrap();
        assert_eq!(fsd::get_attr(&mut lend, &note, 16).unwrap(), b"blue");
        // The server's refusal is its own code.
        assert_eq!(fsd::set_attr(&mut lend, &note, 7, b"x"), Err(Error::Server(ErrorCode::Refused.code())));
        let fids = |files: &[&redoubt_client::file::File]| files.iter().map(|f| f.fid()).collect::<Vec<_>>();
        assert_eq!(
            *seen.lock().unwrap(),
            [
                ("rename", fids(&[&a, &b])),
                ("copy_file", fids(&[&note, &b])),
                ("set_attr", fids(&[&note])),
                ("get_attr", fids(&[&note])),
                ("set_attr", fids(&[&note])),
            ]
        );
    });
    served.stop();
}

/// The attack: files on two connections, one of them a stranger's view of the same server,
/// cannot be joined in one operation: refused here, and the server hears nothing.
#[test]
fn files_on_two_connections_are_refused_before_any_call() {
    let seen = Seen::default();
    let served = start(seen.clone());
    let (client, first) = served.client(1001, 1);
    let second = fake().grant(served.server, served.receive, client, 2);
    fake().as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let one = Connection::attach(Endpoint::from_handle(first), &mut lend).unwrap();
        let two = Connection::attach(Endpoint::from_handle(second), &mut lend).unwrap();
        let a = one.open(&mut lend, "home/a", mode::OREAD).unwrap();
        let b = two.open(&mut lend, "home/b", mode::OREAD).unwrap();
        let other = Err(Error::Refused(Refusal::OtherConnection));
        assert_eq!(fsd::rename(&mut lend, &a, "note", &b, "stolen"), other);
        assert_eq!(fsd::copy_file(&mut lend, &a, &b, "stolen"), other.map(|()| 0));
    });
    assert!(seen.lock().unwrap().is_empty());
    served.stop();
}
