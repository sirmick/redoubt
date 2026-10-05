//! Module and application lookups on the verified userland volume, through Redoubt's real
//! Platform adapter: found, absent and refused reach the VM as themselves.

#![cfg(feature = "fake")]

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use beamlet_redoubt::Redoubt;
use beamlet_redoubt::fixture::{self, HostThreads};
use beamlet_redoubt::userland::{Disk, Files, Unread};
use beamlet_vm::platform::{Lookup, Platform};
use redoubt_fake_kernel::fake;

#[derive(Clone, Default)]
struct Screen(Arc<Mutex<Vec<u8>>>);

impl Write for Screen {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

/// The volume's files by name; a name in `failing` opens and then fails its read, as a block
/// `verityd` refuses does.
struct Volume {
    files: HashMap<String, Vec<u8>>,
    failing: Vec<&'static str>,
    reads: Arc<AtomicUsize>,
}

impl Files for Volume {
    fn read(&mut self, name: &str) -> Result<Vec<u8>, Unread> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        if self.failing.contains(&name) {
            return Err(Unread::Failed);
        }
        self.files.get(name).cloned().ok_or(Unread::Absent)
    }
}

/// A volume with one good file and one that fails, for both lookup types.
fn source() -> (Disk<Volume>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut files = HashMap::new();
    for (name, bytes) in [("good.beam", &b"module good"[..]), ("good.app", b"app good")] {
        files.insert(name.to_string(), bytes.to_vec());
    }
    let failing = vec!["bad.beam", "bad.app"];
    (Disk::new(Volume { files, failing, reads: Arc::clone(&reads) }), reads)
}

fn with_platform(test: impl FnOnce(&mut Redoubt, &AtomicUsize) + Send + 'static) -> String {
    let f = fake();
    let screen = Screen::default();
    let console = fixture::console(Box::new(std::io::empty()), Box::new(screen.clone()));
    let (pid, block) = fixture::session(&console);
    let (modules, reads) = source();
    let session = f.run(pid, move || {
        let startup = fixture::startup(&block);
        let mut platform = Redoubt::new(&startup, Box::new(HostThreads { pid }), Box::new(modules)).unwrap();
        test(&mut platform, &reads);
        0
    });
    assert_eq!(session.join().unwrap(), 0);
    f.destroy(console.pid, console.endpoint);
    assert_eq!(console.thread.join().unwrap(), redoubt_rt::exit::OK);
    let bytes = screen.0.lock().unwrap().clone();
    String::from_utf8(bytes).unwrap()
}

fn check_lookups(
    platform: &mut Redoubt,
    reads: &AtomicUsize,
    lookup: impl Fn(&mut Redoubt, &str) -> Lookup,
    found: &[u8],
) {
    assert_eq!(lookup(platform, "absent"), Lookup::Absent);
    assert_eq!(lookup(platform, "good"), Lookup::Found(found.to_vec()));
    assert_eq!(lookup(platform, "bad"), Lookup::Refused);
    assert_eq!(reads.load(Ordering::Relaxed), 3, "each name is read once");
}

#[test]
fn verified_module_lookup_propagates_found_absent_and_refused() {
    let screen = with_platform(|platform, reads| {
        check_lookups(platform, reads, Redoubt::load_module, b"module good");
    });
    assert_eq!(screen, "beamlet: bad not loaded: its file could not be read\n");
}

#[test]
fn verified_application_lookup_propagates_found_absent_and_refused() {
    let screen = with_platform(|platform, reads| {
        check_lookups(platform, reads, Redoubt::load_app, b"app good");
    });
    assert_eq!(screen, "beamlet: bad not loaded: its file could not be read\n");
}
