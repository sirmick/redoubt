//! File operations a platform finishes later (docs/userland/beamlet.md, "Asynchronous underneath,
//! synchronous on top"): the process that asked waits, not the scheduler; the answer reaches the
//! process that asked and no other; a message does not end the wait; a process killed while it
//! waits has its operation dropped. The fixture's source is `src/io_wait.erl`.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use beamlet_vm::Vm;
use beamlet_vm::platform::{FileError, FileInfo, Files, Lookup, OpenMode, Platform, PlatformError, SeekFrom};

/// What the platform saw, for the test to read back.
#[derive(Default)]
struct Seen {
    /// Each operation begun: its asker and path.
    begun: Vec<(u64, String)>,
    abandoned: Vec<u64>,
}

/// A platform whose whole-file reads finish later: begun when first asked, finished when the VM
/// idles, in the reverse order they were asked, all but `/never`.
struct Deferred {
    now: u64,
    asker: Option<u64>,
    pending: Vec<(u64, String)>,
    done: BTreeMap<u64, Vec<u8>>,
    finished: VecDeque<u64>,
    seen: Arc<Mutex<Seen>>,
}

impl Platform for Deferred {
    fn monotonic_us(&mut self) -> u64 {
        self.now += 1;
        self.now
    }

    fn system_time_us(&mut self) -> Option<u64> { None }

    fn idle(&mut self, deadline: Option<u64>) {
        if let Some(d) = deadline {
            self.now = self.now.max(d);
        }
        let (never, due): (Vec<_>, Vec<_>) = self.pending.drain(..).partition(|(_, path)| path == "/never");
        self.pending = never;
        for (asker, path) in due.into_iter().rev() {
            self.done.insert(asker, format!("contents of {path}").into_bytes());
            self.finished.push_back(asker);
        }
    }

    fn console_write(&mut self, _bytes: &[u8]) {}

    fn random(&mut self, _buf: &mut [u8]) -> Result<(), PlatformError> { Err(PlatformError::Unavailable) }

    fn load_module(&mut self, module: &str) -> Lookup {
        match module {
            "io_wait" => Lookup::Found(include_bytes!("fixtures/io_wait.beam").to_vec()),
            "prim_file" => Lookup::Found(include_bytes!("fixtures/prim_file.beam").to_vec()),
            _ => Lookup::Absent,
        }
    }

    fn files(&mut self) -> Option<&mut dyn Files> { Some(self) }
}

impl Files for Deferred {
    fn read_file(&mut self, path: &str, _max: usize) -> Result<Vec<u8>, FileError> {
        let Some(asker) = self.asker else { return Err(FileError::Eio) };
        if let Some(bytes) = self.done.remove(&asker) {
            return Ok(bytes);
        }
        if !self.pending.iter().any(|(a, _)| *a == asker) {
            self.pending.push((asker, path.to_string()));
            self.seen.lock().unwrap().begun.push((asker, path.to_string()));
        }
        Err(FileError::Later)
    }

    fn asker(&mut self, asker: Option<u64>) { self.asker = asker; }

    fn finished(&mut self) -> Option<u64> { self.finished.pop_front() }

    fn abandon(&mut self, asker: u64) {
        self.pending.retain(|(a, _)| *a != asker);
        self.done.remove(&asker);
        self.seen.lock().unwrap().abandoned.push(asker);
    }

    fn open(&mut self, _: &str, _: OpenMode) -> Result<u64, FileError> { Err(FileError::Enotsup) }

    fn close(&mut self, _: u64) {}

    fn read(&mut self, _: u64, _: usize) -> Result<Vec<u8>, FileError> { Err(FileError::Enotsup) }

    fn write(&mut self, _: u64, _: &[u8]) -> Result<(), FileError> { Err(FileError::Enotsup) }

    fn pread(&mut self, _: u64, _: u64, _: usize) -> Result<Vec<u8>, FileError> { Err(FileError::Enotsup) }

    fn pwrite(&mut self, _: u64, _: u64, _: &[u8]) -> Result<(), FileError> { Err(FileError::Enotsup) }

    fn seek(&mut self, _: u64, _: SeekFrom) -> Result<u64, FileError> { Err(FileError::Enotsup) }

    fn truncate(&mut self, _: u64) -> Result<(), FileError> { Err(FileError::Enotsup) }

    fn sync(&mut self, _: u64) -> Result<(), FileError> { Err(FileError::Enotsup) }

    fn handle_info(&mut self, _: u64) -> Result<FileInfo, FileError> { Err(FileError::Enotsup) }

    fn info(&mut self, _: &str, _: bool) -> Result<FileInfo, FileError> { Err(FileError::Enotsup) }

    fn list_dir(&mut self, _: &str) -> Result<Vec<Vec<u8>>, FileError> { Err(FileError::Enotsup) }

    fn make_dir(&mut self, _: &str) -> Result<(), FileError> { Err(FileError::Enotsup) }

    fn delete(&mut self, _: &str) -> Result<(), FileError> { Err(FileError::Enotsup) }

    fn del_dir(&mut self, _: &str) -> Result<(), FileError> { Err(FileError::Enotsup) }

    fn rename(&mut self, _: &str, _: &str) -> Result<(), FileError> { Err(FileError::Enotsup) }
}

/// Runs `io_wait:f()` and returns its result as text, and what the platform saw.
fn run(f: &str) -> (String, Seen) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let platform = Deferred {
        now: 0,
        asker: None,
        pending: Vec::new(),
        done: BTreeMap::new(),
        finished: VecDeque::new(),
        seen: Arc::clone(&seen),
    };
    let mut vm = Vm::new(Box::new(platform));
    let pid = vm.spawn("io_wait", f, |_| Vec::new()).unwrap();
    let result = vm.run_bounded(pid, 1_000_000).expect("finished").unwrap().unwrap().to_string();
    drop(vm);
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    (result, seen)
}

/// A binary as the VM prints it.
fn text(s: &str) -> String {
    let bytes: Vec<String> = s.bytes().map(|b| b.to_string()).collect();
    format!("<<{}>>", bytes.join(","))
}

#[test]
fn a_completion_reaches_the_process_that_asked_and_no_other() {
    let (result, seen) = run("two");
    // Finished in the reverse order they were asked, each answer still reaches its own asker.
    assert_eq!(result, format!("{{{{ok,{}}},{{ok,{}}}}}", text("contents of /a"), text("contents of /b")));
    // Each operation began once, however often its native was called again.
    let paths: Vec<&str> = seen.begun.iter().map(|(_, p)| p.as_str()).collect();
    assert_eq!(paths, ["/a", "/b"]);
    assert_ne!(seen.begun[0].0, seen.begun[1].0);
}

#[test]
fn a_native_entered_through_its_stubs_body_waits_too() {
    // OTP's prim_file calls its NIFs locally: the stub's body is the native.
    let (result, seen) = run("local");
    assert_eq!(result, format!("{{{{ok,{}}},{{ok,{}}}}}", text("contents of /a"), text("contents of /b")));
    assert_eq!(seen.begun.len(), 2);
}

#[test]
fn a_message_does_not_end_a_wait_for_a_file() {
    let (result, seen) = run("messages");
    assert_eq!(result, format!("{{ok,{}}}", text("contents of /a")));
    assert_eq!(seen.begun.len(), 1);
}

#[test]
fn a_process_killed_while_it_waits_has_its_operation_dropped() {
    let (result, seen) = run("killed");
    assert_eq!(result, "false");
    assert_eq!(seen.begun.len(), 1);
    assert_eq!(seen.abandoned, [seen.begun[0].0]);
}
