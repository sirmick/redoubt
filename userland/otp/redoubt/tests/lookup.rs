//! Verified module and application lookups through Redoubt's real Platform adapter.

#![cfg(feature = "fake")]

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use beamlet_redoubt::Redoubt;
use beamlet_redoubt::fixture::{self, HostThreads};
use beamlet_redoubt::userland::{Checked, Index, Objects, object_name};
use beamlet_vm::platform::{Lookup, Platform};
use redoubt_client::Error;
use redoubt_fake_kernel::fake;
use sha2::{Digest, Sha256};

#[derive(Clone, Default)]
struct Screen(Arc<Mutex<Vec<u8>>>);

impl Write for Screen {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

struct Disk {
    files: HashMap<String, Vec<u8>>,
    reads: Arc<AtomicUsize>,
}

impl Objects for Disk {
    fn read(&mut self, name: &str, max: u64) -> Result<Vec<u8>, Error> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let mut bytes = self.files.get(name).cloned().ok_or(Error::Rerror)?;
        bytes.truncate(max as usize);
        Ok(bytes)
    }
}

fn line(name: &str, bytes: &[u8]) -> String {
    format!("{name} {} {}\n", object_name(&Sha256::digest(bytes).into()), bytes.len())
}

/// A checked source with one good object and each kind of refused object for both lookup types.
fn source() -> (Checked<Disk>, Arc<AtomicUsize>) {
    let cases: [(&str, &[u8]); 10] = [
        ("good.beam", b"module good"),
        ("bad.beam", b"module bad"),
        ("missing.beam", b"module missing"),
        ("short.beam", b"module short"),
        ("long.beam", b"module long"),
        ("good.app", b"app good"),
        ("bad.app", b"app bad"),
        ("missing.app", b"app missing"),
        ("short.app", b"app short"),
        ("long.app", b"app long"),
    ];
    let mut lines: Vec<_> = cases.iter().map(|(name, bytes)| line(name, bytes)).collect();
    lines.sort();
    let reads = Arc::new(AtomicUsize::new(0));
    let mut files = HashMap::new();
    for (name, bytes) in cases {
        let mut stored = bytes.to_vec();
        match name.split('.').next().unwrap() {
            "bad" => stored[0] ^= 1,
            "short" => {
                stored.pop();
            }
            "long" => stored.push(b'!'),
            _ => {}
        }
        if !name.starts_with("missing") {
            files.insert(object_name(&Sha256::digest(bytes).into()), stored);
        }
    }
    let index = Index::parse(lines.concat().as_bytes()).unwrap();
    (Checked::new(index, Disk { files, reads: Arc::clone(&reads) }), reads)
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
    assert_eq!(reads.load(Ordering::Relaxed), 0, "an absent index name reads no object");
    assert_eq!(lookup(platform, "good"), Lookup::Found(found.to_vec()));
    assert_eq!(lookup(platform, "bad"), Lookup::Refused);
    assert_eq!(lookup(platform, "missing"), Lookup::Refused);
    assert_eq!(lookup(platform, "short"), Lookup::Refused);
    assert_eq!(lookup(platform, "long"), Lookup::Refused);
    assert_eq!(reads.load(Ordering::Relaxed), 5, "each indexed name reads once");
}

fn check_diagnostics(screen: &str) {
    assert_eq!(
        screen,
        concat!(
            "beamlet: bad not loaded: its object does not match system.index\n",
            "beamlet: missing not loaded: its object is missing\n",
            "beamlet: short not loaded: its object is short\n",
            "beamlet: long not loaded: its object does not match system.index\n",
        ),
    );
}

#[test]
fn verified_module_lookup_propagates_found_absent_and_refused() {
    let screen = with_platform(|platform, reads| {
        check_lookups(platform, reads, Redoubt::load_module, b"module good");
    });
    check_diagnostics(&screen);
}

#[test]
fn verified_application_lookup_propagates_found_absent_and_refused() {
    let screen = with_platform(|platform, reads| {
        check_lookups(platform, reads, Redoubt::load_app, b"app good");
    });
    check_diagnostics(&screen);
}
