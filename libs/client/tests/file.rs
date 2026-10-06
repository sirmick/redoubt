//! `file`, against the real `bootfsd` and the in-memory file server, on the fake kernel: files
//! read back byte for byte, a refusal is the server's and costs no fid, a connection is shared by
//! threads, and nothing succeeds that the server refused.

mod common;

use std::sync::Arc;

use common::{BOOT, Boot, QUOTA, Served};
use redoubt_client::file::Connection;
use redoubt_client::{Error, Lend, Name, Refusal};
use redoubt_fake_kernel::fake;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::path::MAX_NAME;
use redoubt_rt::server::ninep::{DMDIR, MAX_FIDS, mode};
use redoubt_rt::wire::Error as WireError;

/// A session reads every entry of `/boot` through the library: open, read to the end, stat,
/// the directory's listing, and close.
#[test]
fn a_session_reads_boot_through_the_library() {
    let boot = Boot::start();
    let (session, conn) = boot.session(1001);
    fake().as_process(session, || {
        let mut lend = Lend::new(1).unwrap();
        let conn = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        for (name, data) in BOOT {
            let file = conn.open(&mut lend, name, mode::OREAD).unwrap();
            let mut got = vec![0u8; 64];
            let n = file.read_at(&mut lend, 0, &mut got).unwrap();
            assert_eq!(&got[..n], data, "{name}");
            assert_eq!(file.read_at(&mut lend, n as u64, &mut got).unwrap(), 0, "then the end");
            let stat = file.stat(&mut lend).unwrap();
            assert_eq!((stat.name.as_str(), stat.length), (name, data.len() as u64));
            file.close(&mut lend).unwrap();
        }
        let root = conn.open(&mut lend, "", mode::OREAD).unwrap();
        assert!(root.stat(&mut lend).unwrap().mode & DMDIR != 0);
        let (entries, next) = root.read_dir(&mut lend, 0).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, BOOT.map(|(name, _)| name));
        assert!(root.read_dir(&mut lend, next).unwrap().0.is_empty(), "then no more");
        root.close(&mut lend).unwrap();
        assert_eq!(conn.stat(&mut lend, "beamlet").unwrap().length, 6);
    });
    boot.stop();
}

/// The attack: nothing succeeds that `bootfsd` refuses (the manifest's own name, a write, a
/// create, a remove, a name climbing out of the root), each refusal is `bootfsd`'s own `Rerror`,
/// and none costs a fid: many more refusals than the connection has fids, then an open.
#[test]
fn a_refusal_is_the_servers_and_costs_no_fid() {
    let boot = Boot::start();
    let (session, conn) = boot.session(1001);
    fake().as_process(session, || {
        let mut lend = Lend::new(1).unwrap();
        let conn = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        for _ in 0..2 * MAX_FIDS {
            assert_eq!(
                conn.open(&mut lend, "manifest.json", mode::OREAD).err(),
                Some(Error::Rerror(Name::NotFound))
            );
            assert_eq!(conn.open(&mut lend, "keyd", mode::OWRITE).err(), Some(Error::Rerror(Name::Other)));
            assert_eq!(
                conn.create(&mut lend, "", "new", 0o644, mode::OWRITE).err(),
                Some(Error::Rerror(Name::Other))
            );
            assert_eq!(conn.remove(&mut lend, "keyd"), Err(Error::Rerror(Name::Other)));
        }
        // `..` never climbs above the connection's root: it cleans away, here and in the server.
        let file = conn.open(&mut lend, "../../keyd", mode::OREAD).unwrap();
        file.close(&mut lend).unwrap();
        // A path deeper than one walk is refused here, before any call.
        let deep = "a/".repeat(17);
        assert_eq!(conn.open(&mut lend, &deep, mode::OREAD).err(), Some(Error::Refused(Refusal::BadPath)));
    });
    boot.stop();
}

/// A file dropped without `close` makes no call: its fid stays in use until the connection
/// ends, so dropping every fid the connection has leaves none, while closed files give theirs back.
#[test]
fn a_dropped_file_keeps_its_fid_and_a_closed_one_returns_it() {
    let served = Served::start(|_, request| {
        drop(request);
        Ok(())
    });
    let (client, conn) = served.client(1001, 1);
    fake().as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let conn = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        for _ in 0..3 * MAX_FIDS {
            conn.open(&mut lend, "home/a/note", mode::OREAD).unwrap().close(&mut lend).unwrap();
        }
        let before = fake().calls(client).len();
        // Every fid but the root's.
        let kept: Vec<_> =
            (1..MAX_FIDS).map(|_| conn.open(&mut lend, "home/a/note", mode::OREAD).unwrap()).collect();
        drop(kept);
        assert_eq!(
            fake().calls(client).len(),
            before + 2 * (MAX_FIDS - 1),
            "a walk and an open each, no clunk"
        );
        assert_eq!(
            conn.open(&mut lend, "home/a/note", mode::OREAD).err(),
            Some(Error::Refused(Refusal::NoFid))
        );
    });
    served.stop();
}

/// The attack: a path that cleans but is too long for the lend (16 names of 255 bytes: a `Twalk`
/// of 4129 bytes, in one page) is refused before it is sent, and costs no fid. Many more of them
/// than the connection has fids, and the fake kernel's log shows no call made; then an open still
/// succeeds.
#[test]
fn a_path_too_long_to_send_costs_no_fid() {
    let served = Served::start(|_, request| {
        drop(request);
        Ok(())
    });
    let (client, conn) = served.client(1001, 1);
    fake().as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let conn = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        let long = vec!["a".repeat(MAX_NAME); 16].join("/");
        let before = fake().calls(client).len();
        for _ in 0..2 * MAX_FIDS {
            assert_eq!(
                conn.open(&mut lend, &long, mode::OREAD).err(),
                Some(Error::Wire(WireError::TooLarge))
            );
        }
        assert_eq!(fake().calls(client).len(), before, "nothing was sent");
        conn.open(&mut lend, "home/a/note", mode::OREAD).unwrap().close(&mut lend).unwrap();
    });
    served.stop();
}

/// Create, write, read back, list and remove, through the library on a server that allows them.
#[test]
fn files_are_created_written_and_removed() {
    let served = Served::start(|_, request| {
        drop(request);
        Ok(())
    });
    let (client, conn) = served.client(1001, 1);
    fake().as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let conn = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        let file = conn.create(&mut lend, "home/a", "draft", 0o644, mode::ORDWR).unwrap();
        assert_eq!(file.write_at(&mut lend, 0, b"first words").unwrap(), 11);
        let mut got = [0u8; 32];
        let n = file.read_at(&mut lend, 0, &mut got).unwrap();
        assert_eq!(&got[..n], b"first words");
        file.close(&mut lend).unwrap();
        let dir = conn.open(&mut lend, "home/a", mode::OREAD).unwrap();
        let names: Vec<_> = dir.read_dir(&mut lend, 0).unwrap().0.into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["note", "draft"]);
        dir.close(&mut lend).unwrap();
        conn.remove(&mut lend, "home/a/draft").unwrap();
        assert_eq!(conn.stat(&mut lend, "home/a/draft").err(), Some(Error::Rerror(Name::NotFound)));
        // Twice is refused, by the server.
        assert_eq!(conn.remove(&mut lend, "home/a/draft"), Err(Error::Rerror(Name::NotFound)));
    });
    served.stop();
}

/// The attacks on a minted connection: rooted below its parent's root, it cannot walk out of it,
/// and a quota the server refuses mints nothing.
#[test]
fn a_minted_connection_cannot_climb_out_of_its_root_and_a_refused_quota_mints_nothing() {
    let served = Served::start(|_, request| {
        drop(request);
        Ok(())
    });
    let (launcher, conn) = served.client(0, 1);
    let child = fake().process(1001, &[]);
    let held = fake().held(launcher).0;
    let minted = fake().as_process(launcher, || {
        let mut lend = Lend::new(1).unwrap();
        let conn = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        assert_eq!(
            conn.new_connection(&mut lend, "home/a", QUOTA + 1).err(),
            Some(Error::Rerror(Name::Other))
        );
        assert_eq!(fake().held(launcher).0, held, "a refused quota brought no handle");
        conn.new_connection(&mut lend, "home/a", QUOTA).unwrap().0
    });
    let minted = fake().copy(launcher, minted.handle(), child);
    fake().as_process(child, || {
        let mut lend = Lend::new(1).unwrap();
        let conn = Connection::attach(Endpoint::from_handle(minted), &mut lend).unwrap();
        assert_eq!(conn.stat(&mut lend, "note").unwrap().length, 5);
        // Every way up cleans back to its own root, where `b` is not.
        for path in ["../b/secret", "../../home/b/secret", "/../b/secret", "note/../../b/secret"] {
            assert_eq!(
                conn.open(&mut lend, path, mode::OREAD).err(),
                Some(Error::Rerror(Name::NotFound)),
                "{path}"
            );
        }
        let root = conn.open(&mut lend, "..", mode::OREAD).unwrap();
        let names: Vec<_> = root.read_dir(&mut lend, 0).unwrap().0.into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["note"], "`..` of its root is its root");
    });
    served.stop();
}

/// Threads share one connection: each opens, reads and closes its own files at once, from one
/// fid allocator, and none ever gets a fid another holds.
#[test]
fn threads_share_a_connection() {
    let boot = Boot::start();
    let (session, conn) = boot.session(1001);
    let shared = fake().as_process(session, || {
        let mut lend = Lend::new(1).unwrap();
        Arc::new(Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap())
    });
    let threads: Vec<_> = BOOT
        .iter()
        .map(|&(name, data)| {
            let conn = Arc::clone(&shared);
            fake().run(session, move || {
                let mut lend = Lend::new(1).unwrap();
                for _ in 0..50 {
                    let file = conn.open(&mut lend, name, mode::OREAD).unwrap();
                    let mut got = [0u8; 16];
                    let n = file.read_at(&mut lend, 0, &mut got).unwrap();
                    assert_eq!(&got[..n], data, "{name} read another file");
                    file.close(&mut lend).unwrap();
                }
                0
            })
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap(), 0);
    }
    boot.stop();
}

/// A connection whose server has gone is `Disconnected` on every call, and is not reconnected.
#[test]
fn a_gone_server_is_disconnected_every_time() {
    let boot = Boot::start();
    let (session, conn) = boot.session(1001);
    let conn = fake().as_process(session, || {
        let mut lend = Lend::new(1).unwrap();
        Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap()
    });
    let held = fake().held(session).0;
    boot.stop();
    fake().as_process(session, || {
        let mut lend = Lend::new(1).unwrap();
        for _ in 0..3 {
            assert_eq!(conn.open(&mut lend, "keyd", mode::OREAD).err(), Some(Error::Disconnected));
        }
    });
    assert_eq!(fake().held(session).0, held, "and no handle came of it");
}

/// An `Rerror` keeps its name, never its text (servers/wire.md, "Error names"): a name that is
/// not there, whether the server answers `file does not exist` or walks only part of the path, is
/// `not_found`; any other refusal is `Other`. A reader tells a missing file from a refused one by
/// this alone (fsd's `corrupt` is `Other`: servers/fsd/tests/fsd.rs).
#[test]
fn an_rerror_keeps_its_name_not_found_against_the_rest() {
    let served = Served::start(|_, request| {
        drop(request);
        Ok(())
    });
    let (client, conn) = served.client(1001, 1);
    fake().as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let conn = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        // The first name is not there: the server's `file does not exist`.
        assert_eq!(conn.open(&mut lend, "nowhere", mode::OREAD).err(), Some(Error::Rerror(Name::NotFound)));
        // The walk stops after `home`: a short `Rwalk`, not an `Rerror`, and the same name.
        assert_eq!(conn.stat(&mut lend, "home/nobody").err(), Some(Error::Rerror(Name::NotFound)));
        // `home` is there but not empty: the server's `permission denied`.
        assert_eq!(conn.remove(&mut lend, "home"), Err(Error::Rerror(Name::Other)));
        conn.open(&mut lend, "home/a/note", mode::OREAD).unwrap().close(&mut lend).unwrap();
    });
    served.stop();
}
