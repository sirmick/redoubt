//! `beamlet`: run BEAM code on a POSIX host.
//!
//!     beamlet [-pa DIR]... [--root DIR] MODULE [FUNCTION [ARG...]]
//!     beamlet --check FILE.beam...      validate files with the loader and report errors
//!
//! Loads modules on demand from the `-pa` directories (in order), calls `MODULE:FUNCTION()`
//! (default `start`) in a new process, or `MODULE:FUNCTION([ARG, ...])` with the arguments as
//! strings when there are any, and prints its result with `~w` formatting:
//! the returned term, or `{'EXCEPTION',Class,Reason}`. The differential test harness compares
//! this line with what the real BEAM prints for the same call.
//!
//! `--root DIR` gives the VM a file system: `DIR` becomes its `/`, and nothing outside it is
//! reachable. Without it, `file` operations fail with `enotsup`. `--mount /AT=DIR[:ro]` shows
//! another host directory at `/AT` (read-only with `:ro`); `--lib /DIR` names a directory of
//! the VM's file system holding applications (`App-Vsn/...`) for `code:lib_dir/1`.

mod files;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use std::sync::mpsc::{Receiver, RecvTimeoutError};

use beamlet_vm::platform::{ConsoleInput, Platform, PlatformError};
use beamlet_vm::{Class, Term, Vm};

impl Posix {
    /// The first `name` in the code path. Names come from module and application atoms:
    /// refuse anything that could leave the directory.
    fn find(&self, name: &str) -> Option<Vec<u8>> {
        std::fs::read(self.locate(name)?).ok()
    }

    /// The host path of the first `name` in the code path.
    fn locate(&self, name: &str) -> Option<PathBuf> {
        if name.is_empty() || name.contains(['/', '\\', '\0']) || name.starts_with('.') {
            return None;
        }
        self.code_path.iter().map(|dir| dir.join(name)).find(|p| p.is_file())
    }
}

/// The POSIX platform: a monotonic clock, stdout as the console, `getrandom` via `/dev/urandom`,
/// and `.beam` files from a search path.
struct Posix {
    start: Instant,
    code_path: Vec<PathBuf>,
    files: Option<files::HostDir>,
    /// Console input from a thread reading stdin, started when the VM first asks for input.
    input: Option<Receiver<ConsoleInput>>,
    /// Input that arrived while `idle` was waiting, for the next `console_read`.
    stash: Option<ConsoleInput>,
    /// Whether the console output so far ends mid-line (after a prompt, say).
    mid_line: std::rc::Rc<std::cell::Cell<bool>>,
}

/// Read stdin on its own thread, so the VM never blocks on it.
fn stdin_reader() -> Receiver<ConsoleInput> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = [0u8; 4096];
        let mut stdin = std::io::stdin().lock();
        loop {
            let msg = match stdin.read(&mut buf) {
                Ok(0) | Err(_) => ConsoleInput::Eof,
                Ok(n) => ConsoleInput::Data(buf[..n].to_vec()),
            };
            let end = msg == ConsoleInput::Eof;
            if tx.send(msg).is_err() || end {
                return;
            }
        }
    });
    rx
}

impl Platform for Posix {
    fn monotonic_us(&mut self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }

    fn system_time_us(&mut self) -> Option<u64> {
        SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_micros() as u64)
    }

    fn idle(&mut self, deadline: Option<u64>) {
        // The only external event is console input; without a reader there is only the clock.
        let wait = deadline.map(|d| std::time::Duration::from_micros(d.saturating_sub(self.monotonic_us())));
        match (&self.input, self.stash.is_some()) {
            (Some(rx), false) => {
                let got = match wait {
                    Some(w) => rx.recv_timeout(w),
                    None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                };
                self.stash = Some(match got {
                    Ok(msg) => msg,
                    Err(RecvTimeoutError::Timeout) => ConsoleInput::Nothing,
                    Err(RecvTimeoutError::Disconnected) => ConsoleInput::Eof,
                });
            }
            (Some(_), true) => {}
            (None, _) => {
                if let Some(w) = wait {
                    std::thread::sleep(w);
                }
            }
        }
    }

    fn console_read(&mut self) -> ConsoleInput {
        if let Some(msg) = self.stash.take() {
            return msg;
        }
        let rx = self.input.get_or_insert_with(stdin_reader);
        match rx.try_recv() {
            Ok(msg) => msg,
            Err(std::sync::mpsc::TryRecvError::Empty) => ConsoleInput::Nothing,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => ConsoleInput::Eof,
        }
    }

    fn console_write(&mut self, bytes: &[u8]) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(bytes);
        let _ = out.flush();
        if let Some(&last) = bytes.last() {
            self.mid_line.set(last != b'\n');
        }
    }

    fn random(&mut self, buf: &mut [u8]) -> Result<(), PlatformError> {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(buf))
            .map_err(|_| PlatformError::Unavailable)
    }

    fn load_module(&mut self, module: &str) -> Option<Vec<u8>> {
        self.find(&format!("{module}.beam"))
    }

    fn load_app(&mut self, app: &str) -> Option<Vec<u8>> {
        self.find(&format!("{app}.app"))
    }

    fn module_file(&mut self, module: &str) -> Option<String> {
        let host = self.locate(&format!("{module}.beam"))?;
        self.files.as_ref()?.vm_path(&host)
    }

    fn files(&mut self) -> Option<&mut dyn beamlet_vm::platform::Files> {
        self.files.as_mut().map(|f| f as &mut dyn beamlet_vm::platform::Files)
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: beamlet [-pa DIR]... [--root DIR [--mount /AT=DIR[:ro]]... [--lib /DIR]...] MODULE [FUNCTION [ARG...]]");
    ExitCode::from(2)
}

/// Load each file and print `ok` or the loader's error.
fn check(files: &[String]) -> ExitCode {
    let mut failed = false;
    for f in files {
        let result = std::fs::read(f)
            .map_err(|e| format!("{e}"))
            .and_then(|b| beamlet_vm::loader::load(&b, &mut beamlet_vm::atom::AtomTable::new()).map_err(|e| format!("{e:?}")));
        match result {
            Ok(m) => println!("{f}: ok ({}, {} instructions)", m.name.as_str(), m.code.len()),
            Err(e) => {
                println!("{f}: {e}");
                failed = true;
            }
        }
    }
    if failed { ExitCode::from(1) } else { ExitCode::SUCCESS }
}

fn main() -> ExitCode {
    let all: Vec<String> = std::env::args().skip(1).collect();
    if all.first().map(String::as_str) == Some("--check") {
        return check(&all[1..]);
    }
    let mut args = std::env::args().skip(1);
    let mut code_path = Vec::new();
    let mut positional = Vec::new();
    let mut root = None;
    let mut mounts = Vec::new();
    let mut libs = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--mount" => match args.next() {
                Some(spec) => mounts.push(spec),
                None => return usage(),
            },
            "--lib" => match args.next() {
                Some(dir) => libs.push(dir),
                None => return usage(),
            },
            "--root" => match args.next().map(|d| files::HostDir::new(&d).map_err(|e| (d, e))) {
                Some(Ok(dir)) => root = Some(dir),
                Some(Err((d, e))) => {
                    eprintln!("beamlet: --root {d}: {e}");
                    return ExitCode::from(2);
                }
                None => return usage(),
            },
            "-pa" => match args.next() {
                Some(dir) => code_path.push(PathBuf::from(dir)),
                None => return usage(),
            },
            _ => positional.push(a),
        }
    }
    let (module, function, args) = match positional.as_slice() {
        [m] => (m.clone(), "start".to_string(), None),
        [m, f] => (m.clone(), f.clone(), None),
        [m, f, rest @ ..] => (m.clone(), f.clone(), Some(rest.to_vec())),
        _ => return usage(),
    };

    for spec in &mounts {
        let Some(fs) = root.as_mut() else {
            eprintln!("beamlet: --mount needs --root");
            return ExitCode::from(2);
        };
        let (at, host) = spec.split_once('=').unwrap_or((spec, ""));
        let (host, ro) = match host.strip_suffix(":ro") {
            Some(h) => (h, true),
            None => (host, false),
        };
        if let Err(e) = fs.mount(at, host, ro) {
            eprintln!("beamlet: --mount {spec}: {e}");
            return ExitCode::from(2);
        }
    }
    let has_root = root.is_some();
    let mid_line = std::rc::Rc::new(std::cell::Cell::new(false));
    let platform = Posix { start: Instant::now(), code_path, files: root, input: None, stash: None, mid_line: mid_line.clone() };
    // Natives are 'static slices; join the crates' tables once.
    let natives: &'static [beamlet_vm::bif::NativeSpec] =
        Box::leak([beamlet_crypto::NATIVES, beamlet_re::NATIVES].concat().into_boxed_slice());
    let config = beamlet_vm::vm::Config { natives, ..Default::default() };
    let mut vm = Vm::with_config(Box::new(platform), config);
    for dir in &libs {
        vm.add_lib_root(dir);
    }
    // With a file system, the VM's home is its root (the host's is not visible).
    if has_root {
        vm.setenv("HOME", "/");
    }
    let string = |s: &str| Term::list(s.chars().map(|c| Term::Int(c as i64)).collect::<Vec<_>>());
    let call_args = match args {
        Some(a) => vec![Term::list(a.iter().map(|s| string(s)).collect::<Vec<_>>())],
        None => Vec::new(),
    };
    let pid = match vm.spawn(&module, &function, call_args) {
        Ok(pid) => pid,
        Err(e) => {
            println!("{}", exception(&mut vm, e.class, e.reason));
            return ExitCode::SUCCESS;
        }
    };
    // BEAMLET_PROFILE=N: print the N hottest places (sampled once per time slice) at exit.
    let profile = std::env::var("BEAMLET_PROFILE").ok().and_then(|n| n.parse::<usize>().ok());
    if profile.is_some() {
        vm.enable_profile();
    }
    let result = vm.run(pid);
    if let Some(n) = profile {
        let samples = vm.profile();
        let total: u64 = samples.iter().map(|s| s.0).sum();
        eprintln!("profile: {total} samples");
        for (count, place) in samples.iter().take(n) {
            eprintln!("{:6.2}% {count:6} {place}", 100.0 * *count as f64 / total.max(1) as f64);
        }
    }
    // The result goes on a line of its own, even after a prompt.
    if mid_line.get() {
        println!();
    }
    match result {
        Ok(Ok(value)) => println!("{value}"),
        Ok(Err(e)) => {
            if std::env::var_os("BEAMLET_DEBUG").is_some() {
                eprintln!("stacktrace: {}", e.trace.clone().unwrap_or(Term::Nil));
            }
            println!("{}", exception(&mut vm, e.class, e.reason))
        }
        Err(beamlet_vm::vm::RunError::Halted(status)) => return ExitCode::from(status.clamp(0, 255) as u8),
        Err(e) => {
            eprintln!("beamlet: {e:?}");
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

fn exception(vm: &mut Vm, class: Class, reason: Term) -> Term {
    let class = match class {
        Class::Error => "error",
        Class::Exit => "exit",
        Class::Throw => "throw",
    };
    Term::tuple(vec![vm.atom("EXCEPTION"), vm.atom(class), reason])
}
