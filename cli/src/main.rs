//! `beamlet`: run BEAM code on a POSIX host.
//!
//!     beamlet [-pa DIR]... MODULE [FUNCTION]
//!     beamlet --check FILE.beam...      validate files with the loader and report errors
//!
//! Loads modules on demand from the `-pa` directories (in order), calls `MODULE:FUNCTION()`
//! (default `start`) in a new process, and prints its result with `~w` formatting:
//! the returned term, or `{'EXCEPTION',Class,Reason}`. The differential test harness compares
//! this line with what the real BEAM prints for the same call.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use beamlet_vm::platform::{Platform, PlatformError};
use beamlet_vm::{Class, Term, Vm};

/// The POSIX platform: a monotonic clock, stdout as the console, `getrandom` via `/dev/urandom`,
/// and `.beam` files from a search path.
struct Posix {
    start: Instant,
    code_path: Vec<PathBuf>,
}

impl Platform for Posix {
    fn monotonic_us(&mut self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }

    fn system_time_us(&mut self) -> Option<u64> {
        SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_micros() as u64)
    }

    fn idle(&mut self, deadline: Option<u64>) {
        // With no deadline there is nothing to wait for: this platform has no external events.
        if let Some(d) = deadline {
            let now = self.monotonic_us();
            if d > now {
                std::thread::sleep(std::time::Duration::from_micros(d - now));
            }
        }
    }

    fn console_write(&mut self, bytes: &[u8]) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(bytes);
        let _ = out.flush();
    }

    fn random(&mut self, buf: &mut [u8]) -> Result<(), PlatformError> {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(buf))
            .map_err(|_| PlatformError::Unavailable)
    }

    fn load_module(&mut self, module: &str) -> Option<Vec<u8>> {
        // Module names become file names: refuse anything that could leave the directory.
        if module.is_empty() || module.contains(['/', '\\', '\0']) || module.starts_with('.') {
            return None;
        }
        self.code_path.iter().find_map(|dir| std::fs::read(dir.join(format!("{module}.beam"))).ok())
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: beamlet [-pa DIR]... MODULE [FUNCTION]");
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
    while let Some(a) = args.next() {
        match a.as_str() {
            "-pa" => match args.next() {
                Some(dir) => code_path.push(PathBuf::from(dir)),
                None => return usage(),
            },
            _ => positional.push(a),
        }
    }
    let (module, function) = match positional.as_slice() {
        [m] => (m.clone(), "start".to_string()),
        [m, f] => (m.clone(), f.clone()),
        _ => return usage(),
    };

    let platform = Posix { start: Instant::now(), code_path };
    let mut vm = Vm::new(Box::new(platform));
    let pid = match vm.spawn(&module, &function, Vec::new()) {
        Ok(pid) => pid,
        Err(e) => {
            println!("{}", exception(&mut vm, e.class, e.reason));
            return ExitCode::SUCCESS;
        }
    };
    match vm.run(pid) {
        Ok(Ok(value)) => println!("{value}"),
        Ok(Err(e)) => {
            if std::env::var_os("BEAMLET_DEBUG").is_some() {
                eprintln!("stacktrace: {}", e.trace.clone().unwrap_or(Term::Nil));
            }
            println!("{}", exception(&mut vm, e.class, e.reason))
        }
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
