//! Files over 9P (docs/userland/files.md, "Files over 9P"): the platform's `Files` against the
//! real `littlefsd` on the fake kernel, bound at `/home/alice`, each call made as the VM makes it:
//! as an asker, answered `Later`, waited for in `idle` until the platform names it finished, then
//! made again for its result.

use std::sync::{Arc, Mutex};

use beamlet_redoubt::fixture::{self, Dirs};
use beamlet_redoubt::{Redoubt, binds, posix};
use beamlet_vm::platform::{FileError, FileKind, Files, OpenMode, Platform, SeekFrom};
use redoubt_client::Name;
use redoubt_client::aio::RETRY_US;
use redoubt_fake_kernel::fake;

/// Runs `test` on a platform whose namespace has `/dev/cons` and a blank volume at `/home/alice`.
fn with_home(test: impl FnOnce(&mut Redoubt) + Send + 'static) { with_home_and(None, test); }

/// As [`with_home`], with the fixture's sink server at `/sink` as well when one is given.
fn with_home_and(sink: Option<&fixture::SinkServer>, test: impl FnOnce(&mut Redoubt) + Send + 'static) {
    let f = fake();
    let screen = Arc::new(Mutex::new(Vec::new()));
    let console = fixture::console(Box::new(std::io::empty()), Box::new(Screen(Arc::clone(&screen))));
    let volume = fixture::volume(2048, &["buckets=4"]);
    let sink = sink.map(|s| (s.pid, s.endpoint));
    let (pid, block) = fixture::session_built(&console, &[("/home/alice", &volume)], &[], &[], move |pid| {
        sink.map_or_else(Vec::new, |(server, endpoint)| vec![("/sink", f.grant(server, endpoint, pid, 0x60))])
    });
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        let mut platform = Redoubt::new(&startup, Box::new(Dirs(Vec::new()))).expect("a platform");
        test(&mut platform);
        0
    });
    assert_eq!(session.join().unwrap(), 0);
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
    f.destroy(console.pid, console.endpoint);
    let _ = console.thread.join();
}

struct Screen(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Screen {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

/// The asker the tests' calls are made as.
const ME: u64 = 7;

/// Makes `call` as [`ME`] until it answers, idling between as the VM does.
fn ask<T>(
    p: &mut Redoubt,
    mut call: impl FnMut(&mut Redoubt) -> Result<T, FileError>,
) -> Result<T, FileError> {
    loop {
        p.asker(Some(ME));
        let r = call(p);
        p.asker(None);
        if r.as_ref().err() != Some(&FileError::Later) {
            return r;
        }
        while p.finished() != Some(ME) {
            p.idle(None);
        }
    }
}

fn create(how: OpenMode) -> OpenMode { OpenMode { write: true, create: true, ..how } }

/// Writes `bytes` as the whole of the file at `path`.
fn put(p: &mut Redoubt, path: &str, bytes: &[u8]) {
    let how = create(OpenMode { truncate: true, ..OpenMode::default() });
    let h = ask(p, |p| p.open(path, how)).unwrap();
    ask(p, |p| p.write(h, bytes)).unwrap();
    p.close(h);
}

#[test]
fn files_are_written_read_listed_renamed_and_removed() {
    with_home(|p| {
        put(p, "/home/alice/notes.txt", b"buy milk\n");
        assert_eq!(ask(p, |p| p.read_file("/home/alice/notes.txt", 1 << 20)).unwrap(), b"buy milk\n");
        ask(p, |p| p.make_dir("/home/alice/projects")).unwrap();
        let mut names = ask(p, |p| p.list_dir("/home/alice")).unwrap();
        names.sort();
        assert_eq!(names, [b"notes.txt".to_vec(), b"projects".to_vec()]);
        let info = ask(p, |p| p.info("/home/alice/notes.txt", true)).unwrap();
        assert_eq!((info.size, info.kind), (9, FileKind::Regular));
        assert_eq!(ask(p, |p| p.info("/home/alice/projects", true)).unwrap().kind, FileKind::Directory);
        ask(p, |p| p.rename("/home/alice/notes.txt", "/home/alice/projects/todo.txt")).unwrap();
        assert_eq!(ask(p, |p| p.read_file("/home/alice/projects/todo.txt", 1 << 20)).unwrap(), b"buy milk\n");
        assert_eq!(ask(p, |p| p.del_dir("/home/alice/projects/todo.txt")), Err(FileError::Enotdir));
        assert_eq!(ask(p, |p| p.delete("/home/alice/projects")), Err(FileError::Eisdir));
        ask(p, |p| p.delete("/home/alice/projects/todo.txt")).unwrap();
        ask(p, |p| p.del_dir("/home/alice/projects")).unwrap();
        assert_eq!(ask(p, |p| p.list_dir("/home/alice")).unwrap(), Vec::<Vec<u8>>::new());
    });
}

/// A rename `littlefsd` will not do inside its own volume answers its typed error by the table's
/// name: a directory moved into itself is `not_permitted`, so `eacces`, never a connection's
/// `econnrefused`.
#[test]
fn a_rename_the_volume_refuses_is_eacces() {
    with_home(|p| {
        ask(p, |p| p.make_dir("/home/alice/d")).unwrap();
        ask(p, |p| p.make_dir("/home/alice/d/inner")).unwrap();
        assert_eq!(ask(p, |p| p.rename("/home/alice/d", "/home/alice/d/inner/d")), Err(FileError::Eacces));
        assert_eq!(ask(p, |p| p.list_dir("/home/alice/d")).unwrap(), [b"inner".to_vec()]);
    });
}

#[test]
fn opening_to_write_replaces_and_appending_adds() {
    with_home(|p| {
        put(p, "/home/alice/f", b"a long first version");
        put(p, "/home/alice/f", b"short");
        assert_eq!(ask(p, |p| p.read_file("/home/alice/f", 1 << 20)).unwrap(), b"short");
        let how = create(OpenMode { append: true, ..OpenMode::default() });
        let h = ask(p, |p| p.open("/home/alice/f", how)).unwrap();
        ask(p, |p| p.write(h, b" and more")).unwrap();
        p.close(h);
        assert_eq!(ask(p, |p| p.read_file("/home/alice/f", 1 << 20)).unwrap(), b"short and more");
        let how = create(OpenMode { exclusive: true, ..OpenMode::default() });
        assert_eq!(ask(p, |p| p.open("/home/alice/f", how)), Err(FileError::Eexist));
    });
}

#[test]
fn a_read_past_one_answer_comes_in_pieces() {
    with_home(|p| {
        // Past what one answer carries (64 KiB less its header), so in pieces both ways.
        let big: Vec<u8> = (0..150_000u32).map(|i| (i % 251) as u8).collect();
        put(p, "/home/alice/big", &big);
        assert_eq!(ask(p, |p| p.read_file("/home/alice/big", 1 << 20)).unwrap(), big);
        let h =
            ask(p, |p| p.open("/home/alice/big", OpenMode { read: true, ..OpenMode::default() })).unwrap();
        assert_eq!(ask(p, |p| p.read(h, 200_000)).unwrap(), big);
        assert_eq!(ask(p, |p| p.read(h, 10)).unwrap(), b"");
        p.close(h);
        assert_eq!(ask(p, |p| p.read_file("/home/alice/big", 1000)), Err(FileError::Einval));
    });
}

#[test]
fn the_position_lives_in_the_vm_and_moves_with_reads_and_seeks() {
    with_home(|p| {
        put(p, "/home/alice/abc", b"abcdefghij");
        let h =
            ask(p, |p| p.open("/home/alice/abc", OpenMode { read: true, ..OpenMode::default() })).unwrap();
        assert_eq!(ask(p, |p| p.read(h, 3)).unwrap(), b"abc");
        assert_eq!(ask(p, |p| p.read(h, 3)).unwrap(), b"def");
        assert_eq!(ask(p, |p| p.seek(h, SeekFrom::Start(1))).unwrap(), 1);
        assert_eq!(ask(p, |p| p.read(h, 2)).unwrap(), b"bc");
        assert_eq!(ask(p, |p| p.seek(h, SeekFrom::End(-2))).unwrap(), 8);
        assert_eq!(ask(p, |p| p.read(h, 5)).unwrap(), b"ij");
        assert_eq!(ask(p, |p| p.seek(h, SeekFrom::Current(-20))), Err(FileError::Einval));
        // A read at an offset leaves the position where it was.
        assert_eq!(ask(p, |p| p.pread(h, 4, 2)).unwrap(), b"ef");
        assert_eq!(ask(p, |p| p.read(h, 1)).unwrap(), b"");
        p.close(h);
        assert_eq!(ask(p, |p| p.read(h, 1)), Err(FileError::Ebadf));
    });
}

#[test]
fn a_path_under_no_binding_is_enoent() {
    with_home(|p| {
        assert_eq!(ask(p, |p| p.read_file("/home/bob/x", 1 << 20)), Err(FileError::Enoent));
        assert_eq!(ask(p, |p| p.info("/home/bob", true)), Err(FileError::Enoent));
        assert_eq!(ask(p, |p| p.list_dir("/etc")), Err(FileError::Enoent));
    });
}

#[test]
fn a_path_above_a_binding_is_a_directory_the_namespace_answers() {
    with_home(|p| {
        for path in ["/", "/home"] {
            let info = ask(p, |p| p.info(path, true)).unwrap();
            assert_eq!((info.kind, info.size, info.unix), (FileKind::Directory, 0, false), "{path}");
        }
        assert_eq!(ask(p, |p| p.list_dir("/home")).unwrap(), [b"alice".to_vec()]);
        assert_eq!(ask(p, |p| p.list_dir("/home/.")).unwrap(), [b"alice".to_vec()]);
        assert!(ask(p, |p| p.list_dir("/")).unwrap().contains(&b"home".to_vec()));
        // Nothing in it changes; making it is eexist, as it is there.
        let write = create(OpenMode::default());
        assert_eq!(ask(p, |p| p.open("/home", write)), Err(FileError::Eacces));
        assert_eq!(ask(p, |p| p.make_dir("/home")), Err(FileError::Eexist));
        assert_eq!(ask(p, |p| p.del_dir("/home")), Err(FileError::Eacces));
        assert_eq!(ask(p, |p| p.delete("/home")), Err(FileError::Eacces));
        assert_eq!(ask(p, |p| p.rename("/home", "/house")), Err(FileError::Eacces));
        // A sibling of a binding is neither inside nor above one.
        assert_eq!(ask(p, |p| p.make_dir("/home/bob")), Err(FileError::Enoent));
        assert_eq!(ask(p, |p| p.list_dir("/home/bob")), Err(FileError::Enoent));
    });
}

#[test]
fn two_askers_wait_at_once_and_each_gets_its_own_answer() {
    with_home(|p| {
        put(p, "/home/alice/a", b"for one");
        put(p, "/home/alice/b", b"for two");
        p.asker(Some(1));
        assert_eq!(p.read_file("/home/alice/a", 1 << 20), Err(FileError::Later));
        p.asker(Some(2));
        assert_eq!(p.read_file("/home/alice/b", 1 << 20), Err(FileError::Later));
        p.asker(None);
        let mut finished = Vec::new();
        while finished.len() < 2 {
            match p.finished() {
                Some(asker) => finished.push(asker),
                None => p.idle(None),
            }
        }
        finished.sort();
        assert_eq!(finished, [1, 2]);
        // Each asker's call made again is answered with its own operation's result.
        p.asker(Some(2));
        assert_eq!(p.read_file("/home/alice/b", 1 << 20).unwrap(), b"for two");
        p.asker(Some(1));
        assert_eq!(p.read_file("/home/alice/a", 1 << 20).unwrap(), b"for one");
    });
}

#[test]
fn the_vms_own_calls_wait_in_place() {
    with_home(|p| {
        put(p, "/home/alice/m.beam", b"code");
        // No asker: code loading, which is synchronous in the VM.
        assert_eq!(p.read_file("/home/alice/m.beam", 1 << 20).unwrap(), b"code");
    });
}

#[test]
fn closed_files_give_their_fids_back() {
    with_home(|p| {
        put(p, "/home/alice/f", b"x");
        // Far more opens than a connection has fids: each close's clunk gives its fid back.
        for _ in 0..300 {
            let h =
                ask(p, |p| p.open("/home/alice/f", OpenMode { read: true, ..OpenMode::default() })).unwrap();
            p.close(h);
            let now = p.monotonic_us();
            p.idle(Some(now));
        }
    });
}

/// An asker that dies mid-operation: its operation asks nothing more than the clunk of what it
/// walked, rather than running to its end.
#[test]
fn an_abandoned_operation_stops_at_its_next_answer() {
    with_home(|p| {
        // Three reads' worth, after the walk and the open.
        put(p, "/home/alice/big", &[7; 150_000]);
        p.asker(Some(ME));
        assert_eq!(p.read_file("/home/alice/big", 1 << 20), Err(FileError::Later));
        p.asker(None);
        p.abandon(ME);
        let before = p.requests();
        for _ in 0..20 {
            let soon = p.monotonic_us() + 20_000;
            p.idle(Some(soon));
        }
        assert_eq!(p.requests() - before, 1, "only the walked fid's clunk");
        assert_eq!(p.finished(), None);
    });
}

/// `/dev/cons` is a file of the namespace like any other: an operation on it shares the console's
/// connection with the console's own read and write, and its answer reaches its asker, not the
/// console. (The fixture's console gives a connection one fid, the console's, so it is `emfile`.)
/// A server over its share answers a write `busy`; the write goes again `RETRY_US` later, as the
/// hub's rule for a queued request has it and as the console's does, not at once: a server that
/// stays busy costs the VM a request every retry interval, never a spin against its one page.
#[test]
fn a_write_answered_busy_goes_again_after_the_retry_interval() {
    let sink = fixture::sink(2);
    let (writes, taken) = (Arc::clone(&sink.writes), Arc::clone(&sink.taken));
    with_home_and(Some(&sink), |p| put(p, "/sink/out", b"third time lucky"));
    assert_eq!(taken.lock().unwrap().as_slice(), b"third time lucky");
    let writes = writes.lock().unwrap().clone();
    assert_eq!(writes.len(), 3, "two refused, then taken: {writes:?}");
    for pair in writes.windows(2) {
        assert!(pair[1] - pair[0] >= RETRY_US, "a retry went early: {writes:?}");
    }
    fake().destroy(sink.pid, sink.endpoint);
    let _ = sink.thread.join();
}

/// A closed file's clunk the server answers `busy` goes again after the retry interval, and its
/// fid is the server's until the clunk is served: freed on the `busy` answer, the number would be
/// walked again while the server still held it, and the server would refuse that walk.
#[test]
fn a_close_answered_busy_is_retried_and_its_fid_is_kept_until_the_clunk_is_served() {
    let sink = fixture::sink(0);
    let (hold, clunks) = (Arc::clone(&sink.hold_reads), Arc::clone(&sink.clunks));
    with_home_and(Some(&sink), move |p| {
        let how = OpenMode { write: true, ..OpenMode::default() };
        let held = ask(p, |p| p.open("/sink/out", OpenMode::default())).unwrap();
        let closed = ask(p, |p| p.open("/sink/out", how)).unwrap();
        // A read the sink holds: the connection's one request is out until it is released.
        hold.store(true, std::sync::atomic::Ordering::SeqCst);
        p.asker(Some(ME + 1));
        assert_eq!(p.read(held, 16), Err(FileError::Later));
        p.asker(None);
        // The close's clunk is the second request: `busy`.
        p.close(closed);
        hold.store(false, std::sync::atomic::Ordering::SeqCst);
        // The read ends once the sink serves it again, which the clunk's retry brings about.
        while p.finished() != Some(ME + 1) {
            p.idle(None);
        }
        p.asker(Some(ME + 1));
        assert_eq!(p.read(held, 16), Ok(Vec::new()));
        p.asker(None);
        // A new open walks a fresh fid: with the closed one freed at the `busy` answer, its number
        // would be reused while the server still held it, and the server would refuse the walk.
        let again = ask(p, |p| p.open("/sink/out", how)).unwrap();
        let until = p.monotonic_us() + 500_000;
        while clunks.lock().unwrap().is_empty() && p.monotonic_us() < until {
            let step = p.monotonic_us() + 20_000;
            p.idle(Some(step));
        }
        assert_eq!(clunks.lock().unwrap().len(), 1, "the server served the closed file's clunk once");
        p.close(held);
        p.close(again);
    });
    fake().destroy(sink.pid, sink.endpoint);
    let _ = sink.thread.join();
}

#[test]
fn a_file_operation_on_the_consoles_connection_is_answered() {
    with_home(|p| {
        p.console_write(b"> ");
        assert_eq!(ask(p, |p| p.info("/dev/cons", true)).err(), Some(FileError::Emfile));
        p.console_write(b"still here");
    });
}

#[test]
fn a_stat_reports_only_what_9p_has() {
    with_home(|p| {
        put(p, "/home/alice/f", b"12345");
        let info = ask(p, |p| p.info("/home/alice/f", true)).unwrap();
        assert!(!info.unix);
        assert_eq!(info.size, 5);
    });
}

#[test]
fn what_has_no_9p_field_is_refused_visibly() {
    with_home(|p| {
        put(p, "/home/alice/f", b"x");
        assert_eq!(p.set_permissions("/home/alice/f", 0o600), Err(FileError::Enotsup));
        assert_eq!(p.set_owner("/home/alice/f", -1, -1), Err(FileError::Enotsup));
        assert_eq!(p.set_times("/home/alice/f", 0, 0), Err(FileError::Enotsup));
        assert_eq!(p.make_symlink(b"f", "/home/alice/l"), Err(FileError::Enotsup));
        assert_eq!(p.make_link("/home/alice/f", "/home/alice/l"), Err(FileError::Enotsup));
        let h = ask(p, |p| p.open("/home/alice/f", create(OpenMode::default()))).unwrap();
        assert_eq!(p.truncate(h), Err(FileError::Enotsup));
        p.close(h);
    });
}

/// Within one volume the server copies: the bytes never cross into the VM. A copy onto a name
/// that is there is `eexist`, across two connections `exdev` (the caller copies through the VM),
/// and onto the namespace's own directory `eacces`.
#[test]
fn a_copy_within_one_volume_is_the_servers() {
    with_home(|p| {
        put(p, "/home/alice/a", b"buy milk\n");
        assert_eq!(ask(p, |p| p.copy_file("/home/alice/a", "/home/alice/b")), Ok(9));
        assert_eq!(ask(p, |p| p.read_file("/home/alice/b", 64)).unwrap(), b"buy milk\n");
        assert_eq!(ask(p, |p| p.copy_file("/home/alice/a", "/home/alice/b")), Err(FileError::Eexist));
        assert_eq!(ask(p, |p| p.copy_file("/home/alice/a", "/dev/cons/a")), Err(FileError::Exdev));
        assert_eq!(ask(p, |p| p.copy_file("/home/alice/a", "/home")), Err(FileError::Eacces));
        assert_eq!(ask(p, |p| p.copy_file("/home/alice/none", "/home/alice/c")), Err(FileError::Enoent));
    });
}

/// A directory that is there is `eexist` to make, the namespace's own above the bindings and a
/// binding's root included, never `eacces`: Elixir's `File.mkdir_p` makes each directory from the
/// root down and takes only `eexist` for one that is there.
#[test]
fn a_directory_that_is_there_is_eexist_to_make() {
    with_home(|p| {
        for path in ["/", "/home", "/home/alice"] {
            assert_eq!(ask(p, |p| p.make_dir(path)), Err(FileError::Eexist), "{path}");
        }
        assert_eq!(ask(p, |p| p.make_dir("/home/alice/d")), Ok(()));
        assert_eq!(ask(p, |p| p.make_dir("/home/alice/d")), Err(FileError::Eexist));
    });
}

/// `File.mkdir_p` of a nested new path under a binding, as Elixir 1.20 makes it: each directory
/// from the root down, taking `eexist` for one that is there; the new ones are made, and are
/// directories.
#[test]
fn mkdir_p_of_a_nested_new_path_makes_it() {
    with_home(|p| {
        for path in ["/home", "/home/alice", "/home/alice/a", "/home/alice/a/b", "/home/alice/a/b/c"] {
            match ask(p, |p| p.make_dir(path)) {
                Ok(()) | Err(FileError::Eexist) => {}
                other => panic!("{path}: {other:?}"),
            }
        }
        let info = ask(p, |p| p.info("/home/alice/a/b/c", true)).unwrap();
        assert_eq!(info.kind, FileKind::Directory);
        assert_eq!(ask(p, |p| p.list_dir("/home/alice/a")).unwrap(), [b"b".to_vec()]);
    });
}

/// A field set on a file that is not there is what looking it up finds, never `enotsup`, which
/// OTP's `write_file_info` takes as done: so `File.touch` in a missing directory is not `ok`.
#[test]
fn a_field_set_on_a_missing_file_is_enoent() {
    with_home(|p| {
        assert_eq!(ask(p, |p| p.set_times("/home/alice/none/f", 0, 0)), Err(FileError::Enoent));
        assert_eq!(ask(p, |p| p.set_owner("/home/alice/none", -1, -1)), Err(FileError::Enoent));
        assert_eq!(ask(p, |p| p.set_permissions("/home/bob/f", 0o600)), Err(FileError::Enoent));
    });
}

/// Every row of the one error table as servers/wire.md writes it ("Error names"): each text reads
/// back to the row's name, and the name is the row's POSIX error at the `File` boundary. The page
/// is held to the code, which no compiler does.
#[test]
fn every_row_of_the_error_table_maps_to_its_posix_error() {
    let page = include_str!("../../../../docs/servers/wire.md");
    let table = &page[page.find("| Text | Name | beamlet's `file` error |").expect("the table")..];
    for row in table.lines().skip(2).take_while(|line| line.starts_with('|')) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let (texts, name, atom) = (cells[1], cells[2].trim_matches('`'), cells[3].trim_matches('`'));
        let texts: Vec<&str> = match texts.strip_prefix('`') {
            Some(_) => texts.split(", ").map(|t| t.trim_matches('`')).collect(),
            None => vec!["a server's own words"],
        };
        for text in texts {
            let read = Name::of(text);
            assert_eq!(read.as_str(), name, "{text}");
            assert_eq!(posix(read).name(), atom, "{text}");
        }
    }
}

/// A VM `init` launches has only `/dev/cons` in its namespace; `bind=PREFIX=HANDLE` puts a handle it
/// was handed at a prefix, and nothing it was not handed.
#[test]
fn a_bind_argument_puts_a_handed_volume_in_the_namespace() {
    let f = fake();
    let console = fixture::console(Box::new(std::io::empty()), Box::new(std::io::sink()));
    let volume = fixture::volume(2048, &["buckets=4"]);
    let args =
        ["bind=/home/alice=littlefsd:data", "bind=/home/bob=littlefsd:bob", "bind=home=littlefsd:data"];
    let (pid, block) = fixture::session_with(&console, &[("littlefsd:data", &volume)], &args);
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        // A handle it was not given, and a prefix that is not a clean absolute path: refused.
        assert_eq!(binds(&startup).err(), Some("bind=/home/bob=littlefsd:bob"));
        assert!(Redoubt::new(&startup, Box::new(Dirs(Vec::new()))).is_err());
        0
    });
    assert_eq!(session.join().unwrap(), 0);
    let (pid, block) =
        fixture::session_with(&console, &[("littlefsd:data", &volume)], &["bind=/home/alice=littlefsd:data"]);
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        let mut p = Redoubt::new(&startup, Box::new(Dirs(Vec::new()))).expect("a platform");
        put(&mut p, "/home/alice/x", b"bound");
        assert_eq!(ask(&mut p, |p| p.read_file("/home/alice/x", 64)).unwrap(), b"bound");
        assert_eq!(ask(&mut p, |p| p.info("/home/bob", true)), Err(FileError::Enoent));
        0
    });
    assert_eq!(session.join().unwrap(), 0);
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
    f.destroy(console.pid, console.endpoint);
    let _ = console.thread.join();
}

/// A create the server refuses for any reason but the name being there is that refusal, never an
/// open of a file that is not there: a session labelled `{7}` writing down into an unlabelled volume
/// is refused by the volume's label check, `eacces`, not `enoent`.
#[test]
fn a_create_refused_is_its_refusal() {
    let f = fake();
    let console = fixture::console_labelled(Box::new(std::io::empty()), Box::new(std::io::sink()), &[7]);
    let volume = fixture::volume(2048, &["buckets=4"]);
    let (pid, block) =
        fixture::session_built(&console, &[("/home/alice", &volume)], &[], &[7], |_| Vec::new());
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        let mut p = Redoubt::new(&startup, Box::new(Dirs(Vec::new()))).expect("a platform");
        assert_eq!(ask(&mut p, |p| p.list_dir("/home/alice")), Ok(vec![]));
        let how = create(OpenMode::default());
        assert_eq!(ask(&mut p, |p| p.open("/home/alice/leak", how)), Err(FileError::Eacces));
        assert_eq!(ask(&mut p, |p| p.make_dir("/home/alice/d")), Err(FileError::Eacces));
        0
    });
    assert_eq!(session.join().unwrap(), 0);
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
    f.destroy(console.pid, console.endpoint);
    let _ = console.thread.join();
}
