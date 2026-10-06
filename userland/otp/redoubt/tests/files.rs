//! Files over 9P (docs/userland/files.md, "Files over 9P"): the platform's `Files` against the
//! real `littlefsd` on the fake kernel, bound at `/home/alice`, each call made as the VM makes it:
//! as an asker, answered `Later`, waited for in `idle` until the platform names it finished, then
//! made again for its result.

use std::sync::{Arc, Mutex};

use beamlet_redoubt::fixture::{self, Dirs};
use beamlet_redoubt::{Redoubt, binds, posix};
use beamlet_vm::platform::{FileError, FileKind, Files, OpenMode, Platform, SeekFrom};
use redoubt_client::Name;
use redoubt_fake_kernel::fake;

/// Runs `test` on a platform whose namespace has `/dev/cons` and a blank volume at `/home/alice`.
fn with_home(test: impl FnOnce(&mut Redoubt) + Send + 'static) {
    let f = fake();
    let screen = Arc::new(Mutex::new(Vec::new()));
    let console = fixture::console(Box::new(std::io::empty()), Box::new(Screen(Arc::clone(&screen))));
    let volume = fixture::volume(2048, &["buckets=4"]);
    let (pid, block) = fixture::session_with(&console, &[("/home/alice", &volume)], &[]);
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
        // Nothing in it changes.
        let write = create(OpenMode::default());
        assert_eq!(ask(p, |p| p.open("/home", write)), Err(FileError::Eacces));
        assert_eq!(ask(p, |p| p.make_dir("/home")), Err(FileError::Eacces));
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
        assert_eq!(p.make_symlink(b"f", "/home/alice/l"), Err(FileError::Enotsup));
        assert_eq!(p.make_link("/home/alice/f", "/home/alice/l"), Err(FileError::Enotsup));
        let h = ask(p, |p| p.open("/home/alice/f", create(OpenMode::default()))).unwrap();
        assert_eq!(p.truncate(h), Err(FileError::Enotsup));
        p.close(h);
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
