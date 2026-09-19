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
//!
//! `--exec` lets the VM start host programs behind ports (`open_port/2`, `os:cmd/1`,
//! `System.cmd/3`). They are not sandboxed: see `programs.rs`. `--env NAME[=VALUE]` sets a
//! variable in the VM's environment, which starts with only `HOME` (the host's value when no
//! value is given, e.g. `--env PATH` for programs to be found).

mod files;
mod programs;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};

use beamlet_vm::platform::{
    ConsoleInput, Platform, PlatformError, ProgramEvent, Programs, Spawn, Spawned,
};
use beamlet_vm::term::OwnedTerm;
use beamlet_vm::{Class, Term, Vm};
use programs::Event;

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
        self.code_path
            .iter()
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    }
}

/// The POSIX platform: a monotonic clock, stdout as the console, `getrandom` via `/dev/urandom`,
/// and `.beam` files from a search path.
struct Posix {
    start: Instant,
    code_path: Vec<PathBuf>,
    files: Option<files::HostDir>,
    /// Events from the stdin reader thread and from programs' threads.
    events: Receiver<Event>,
    sender: Sender<Event>,
    /// Whether the stdin reader has been started (when the VM first asks for input), and
    /// whether its input has ended.
    reading: bool,
    input_ended: bool,
    /// Events received but not yet asked for.
    console: VecDeque<ConsoleInput>,
    program_events: VecDeque<(u64, ProgramEvent)>,
    /// Programs started behind ports, if `--exec` allows it.
    programs: Option<programs::Programs>,
    /// Whether the console output so far ends mid-line (after a prompt, say).
    mid_line: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Read stdin on its own thread, so the VM never blocks on it.
fn stdin_reader(tx: Sender<Event>) {
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
            if tx.send(Event::Console(msg)).is_err() || end {
                return;
            }
        }
    });
}

impl Posix {
    fn take(&mut self, event: Event) {
        match event {
            Event::Console(input) => self.console.push_back(input),
            Event::Program(handle, event) => {
                if matches!(event, ProgramEvent::Exit(_)) {
                    if let Some(p) = &mut self.programs {
                        p.exited(handle);
                    }
                }
                self.program_events.push_back((handle, event));
            }
        }
    }

    /// Take every event that has arrived, without waiting.
    fn drain(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            self.take(event);
        }
    }

    /// Whether an event may still arrive.
    fn expecting(&self) -> bool {
        (self.reading && !self.input_ended) || self.programs.as_ref().is_some_and(|p| p.any())
    }
}

impl Platform for Posix {
    fn monotonic_us(&mut self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }

    fn system_time_us(&mut self) -> Option<u64> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_micros() as u64)
    }

    fn idle(&mut self, deadline: Option<u64>) {
        // Wake for console input or a program's output, or at the deadline.
        self.drain();
        if !self.console.is_empty() || !self.program_events.is_empty() {
            return;
        }
        let wait = deadline
            .map(|d| std::time::Duration::from_micros(d.saturating_sub(self.monotonic_us())));
        match wait {
            Some(w) => {
                if let Ok(event) = self.events.recv_timeout(w) {
                    self.take(event);
                }
            }
            // Nothing can arrive, so nothing would wake us: return, and the VM gives up.
            None if !self.expecting() => {}
            None => {
                if let Ok(event) = self.events.recv() {
                    self.take(event);
                }
            }
        }
    }

    fn console_read(&mut self) -> ConsoleInput {
        if !self.reading {
            self.reading = true;
            stdin_reader(self.sender.clone());
        }
        self.drain();
        match self.console.pop_front() {
            Some(ConsoleInput::Eof) => {
                self.input_ended = true;
                ConsoleInput::Eof
            }
            Some(input) => input,
            None if self.input_ended => ConsoleInput::Eof,
            None => ConsoleInput::Nothing,
        }
    }

    fn console_write(&mut self, bytes: &[u8]) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(bytes);
        let _ = out.flush();
        if let Some(&last) = bytes.last() {
            self.mid_line
                .store(last != b'\n', std::sync::atomic::Ordering::Relaxed);
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
        self.files
            .as_mut()
            .map(|f| f as &mut dyn beamlet_vm::platform::Files)
    }

    fn programs(&mut self) -> Option<&mut dyn Programs> {
        if self.programs.is_some() {
            Some(self)
        } else {
            None
        }
    }
}

impl Programs for Posix {
    fn spawn(&mut self, spawn: &Spawn) -> Result<Spawned, beamlet_vm::platform::FileError> {
        let files = &self.files;
        // Without --root the VM's paths are the host's.
        let host = |path: &str| match files {
            Some(fs) => fs.host_path(path),
            None => Ok(PathBuf::from(path)),
        };
        self.programs.as_mut().expect("--exec").spawn(spawn, host)
    }

    fn write(&mut self, handle: u64, data: &[u8]) -> Result<(), beamlet_vm::platform::FileError> {
        self.programs.as_mut().expect("--exec").write(handle, data)
    }

    fn close(&mut self, handle: u64) {
        self.programs.as_mut().expect("--exec").close(handle);
        self.program_events.retain(|(h, _)| *h != handle);
    }

    fn poll(&mut self) -> Option<(u64, ProgramEvent)> {
        self.drain();
        self.program_events.pop_front()
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: beamlet [-pa DIR]... [--root DIR [--mount /AT=DIR[:ro]]... [--lib /DIR]...] [--exec] [--schedulers N] [--env NAME[=VALUE]]... MODULE [FUNCTION [ARG...]]");
    ExitCode::from(2)
}

/// Load each file and print `ok` or the loader's error.
fn check(files: &[String]) -> ExitCode {
    let mut failed = false;
    for f in files {
        let result = std::fs::read(f).map_err(|e| format!("{e}")).and_then(|b| {
            let mut lits = beamlet_vm::term::Literals::default();
            beamlet_vm::loader::load(&b, &mut beamlet_vm::atom::AtomTable::new(), &mut lits)
                .map_err(|e| format!("{e:?}"))
        });
        match result {
            Ok(m) => println!(
                "{f}: ok ({}, {} instructions)",
                m.name.as_str(),
                m.code.len()
            ),
            Err(e) => {
                println!("{f}: {e}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
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
    let mut exec = false;
    let mut env = Vec::new();
    // Schedulers (threads): `--schedulers N`, else $BEAMLET_SCHEDULERS (for test tools), else 1.
    let mut schedulers: usize = std::env::var("BEAMLET_SCHEDULERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--exec" => exec = true,
            "--schedulers" => match args.next().and_then(|n| n.parse().ok()) {
                Some(n) => schedulers = n,
                None => return usage(),
            },
            "--env" => match args.next() {
                Some(spec) => env.push(match spec.split_once('=') {
                    Some((k, v)) => (k.to_string(), Some(v.to_string())),
                    None => (spec, None),
                }),
                None => return usage(),
            },
            "--mount" => match args.next() {
                Some(spec) => mounts.push(spec),
                None => return usage(),
            },
            "--lib" => match args.next() {
                Some(dir) => libs.push(dir),
                None => return usage(),
            },
            "--root" => match args
                .next()
                .map(|d| files::HostDir::new(&d).map_err(|e| (d, e)))
            {
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
    let mid_line = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (sender, events) = std::sync::mpsc::channel();
    let programs = exec.then(|| programs::Programs::new(sender.clone()));
    let platform = Posix {
        start: Instant::now(),
        code_path,
        files: root,
        events,
        sender,
        reading: false,
        input_ended: false,
        console: VecDeque::new(),
        program_events: VecDeque::new(),
        programs,
        mid_line: mid_line.clone(),
    };
    // Natives are 'static slices; join the crates' tables once.
    let natives: &'static [beamlet_vm::bif::NativeSpec] = Box::leak(
        [beamlet_crypto::NATIVES, beamlet_re::NATIVES]
            .concat()
            .into_boxed_slice(),
    );
    let config = beamlet_vm::vm::Config {
        natives,
        ..Default::default()
    };
    let mut vm = Vm::with_config(Box::new(platform), config);
    vm.set_schedulers(schedulers);
    for dir in &libs {
        vm.add_lib_root(dir);
    }
    // With a file system, the VM's home is its root (the host's is not visible).
    if has_root {
        vm.setenv("HOME", "/");
    }
    for (name, value) in env {
        if let Some(value) = value.or_else(|| std::env::var(&name).ok()) {
            vm.setenv(&name, &value);
        }
    }
    let pid = match vm.spawn(&module, &function, |h| match &args {
        Some(a) => {
            let strings: Vec<Term> = a.iter().map(|s| h.string(s)).collect();
            vec![h.list(strings)]
        }
        None => Vec::new(),
    }) {
        Ok(pid) => pid,
        Err(e) => {
            println!("{}", exception(&mut vm, e.class, &e.reason));
            return ExitCode::SUCCESS;
        }
    };
    // BEAMLET_PROFILE=N: print the N hottest places (sampled once per time slice) at exit.
    let profile = std::env::var("BEAMLET_PROFILE")
        .ok()
        .and_then(|n| n.parse::<usize>().ok());
    if profile.is_some() {
        vm.enable_profile();
    }
    let result = vm.run(pid);
    if let Some(n) = profile {
        let samples = vm.profile();
        let total: u64 = samples.iter().map(|s| s.0).sum();
        eprintln!("profile: {total} samples");
        for (count, place) in samples.iter().take(n) {
            eprintln!(
                "{:6.2}% {count:6} {place}",
                100.0 * *count as f64 / total.max(1) as f64
            );
        }
    }
    // The result goes on a line of its own, even after a prompt.
    if mid_line.load(std::sync::atomic::Ordering::Relaxed) {
        println!();
    }
    match result {
        Ok(Ok(value)) => println!("{value}"),
        Ok(Err(e)) => {
            if std::env::var_os("BEAMLET_DEBUG").is_some() {
                match &e.trace {
                    Some(t) => eprintln!("stacktrace: {t}"),
                    None => eprintln!("stacktrace: []"),
                }
            }
            println!("{}", exception(&mut vm, e.class, &e.reason))
        }
        Err(beamlet_vm::vm::RunError::Halted(status)) => {
            return ExitCode::from(status.clamp(0, 255) as u8)
        }
        Err(e) => {
            eprintln!("beamlet: {e:?}");
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

/// `{'EXCEPTION', Class, Reason}`, as the differential harness prints a failed call.
fn exception(vm: &mut Vm, class: Class, reason: &OwnedTerm) -> OwnedTerm {
    let class = match class {
        Class::Error => "error",
        Class::Exit => "exit",
        Class::Throw => "throw",
    };
    let (tag, class) = (vm.atom("EXCEPTION"), vm.atom(class));
    OwnedTerm::build(reason.heap().literals(), |h| {
        let reason = reason.copy_into(h);
        h.tuple(&[tag, class, reason])
    })
}
