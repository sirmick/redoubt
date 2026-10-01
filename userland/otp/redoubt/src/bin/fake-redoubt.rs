//! `fake-redoubt`: beamlet on Redoubt's platform, run on a host, on the fake kernel.
//!
//!     fake-redoubt [-pa DIR]... MODULE [FUNCTION]
//!
//! It starts the fake kernel, a console server on this terminal (the fixture's, see
//! `beamlet_redoubt::fixture`), and a session process whose namespace holds `/dev/cons`; in that
//! process it runs a VM on [`beamlet_redoubt::Redoubt`], calling `MODULE:FUNCTION()` (`start` by
//! default), and prints how it ended. The VM reaches the console through the client library and
//! the fake kernel's IPC, as it will on Redoubt, and has no file system and no programs yet. Its
//! modules come from the `-pa` directories, until they come from `/boot`.

use std::path::PathBuf;
use std::process::ExitCode;

use beamlet_redoubt::Redoubt;
use beamlet_redoubt::fixture::{self, Dirs, HostThreads};
use beamlet_vm::bif::NativeSpec;
use beamlet_vm::vm::Config;
use beamlet_vm::{Class, Vm};
use redoubt_fake_kernel::fake;

/// Eight bytes from this machine's random source.
fn host_seed() -> std::io::Result<u64> {
    use std::io::Read;
    let mut bytes = [0; 8];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn usage() -> ExitCode {
    eprintln!("usage: fake-redoubt [-pa DIR]... MODULE [FUNCTION]");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut dirs = Vec::new();
    let mut positional = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-pa" => match args.next() {
                Some(dir) => dirs.push(PathBuf::from(dir)),
                None => return usage(),
            },
            _ => positional.push(arg),
        }
    }
    let (module, function) = match positional.as_slice() {
        [m] => (m.clone(), String::from("start")),
        [m, f] => (m.clone(), f.clone()),
        _ => return usage(),
    };

    let f = fake();
    // A person at the console may think for as long as they like.
    f.never_stuck();
    // A person's keys, unlike a test's, must not be the same every run.
    match host_seed() {
        Ok(seed) => f.seed_random(seed),
        Err(e) => {
            eprintln!("fake-redoubt: no randomness from this machine: {e}");
            return ExitCode::from(2);
        }
    }
    let console = fixture::console(Box::new(std::io::stdin()), Box::new(std::io::stdout()));
    let (pid, block) = fixture::session(&console);
    let vm = f.run(pid, move || run(&block, pid, dirs, &module, &function));
    let status = vm.join().unwrap_or(1);
    // Ending the console's endpoint ends its server.
    f.destroy(console.pid, console.endpoint);
    let _ = console.thread.join();
    ExitCode::from(u8::try_from(status).unwrap_or(1))
}

/// Runs the VM in the session process, and prints how its first process ended.
fn run(block: &[u8], pid: usize, dirs: Vec<PathBuf>, module: &str, function: &str) -> u32 {
    let startup = fixture::startup(block);
    let platform = match Redoubt::new(&startup, Box::new(HostThreads { pid }), Box::new(Dirs(dirs))) {
        Ok(platform) => platform,
        Err(e) => {
            eprintln!("fake-redoubt: the platform did not start: {e:?}");
            return 1;
        }
    };
    let natives: &'static [NativeSpec] =
        Box::leak([beamlet_crypto::NATIVES, beamlet_re::NATIVES].concat().into_boxed_slice());
    let mut vm = Vm::with_config(Box::new(platform), Config { natives, ..Default::default() });
    let first = match vm.spawn(module, function, |_| Vec::new()) {
        Ok(pid) => pid,
        Err(e) => {
            eprintln!("fake-redoubt: {module}:{function} did not start: {:?} {}", e.class, e.reason);
            return 1;
        }
    };
    match vm.run(first) {
        Ok(Ok(value)) => {
            println!("{value}");
            0
        }
        Ok(Err(e)) => {
            let class = match e.class {
                Class::Error => "error",
                Class::Exit => "exit",
                Class::Throw => "throw",
            };
            println!("{{'EXCEPTION',{class},{}}}", e.reason);
            0
        }
        Err(e) => {
            eprintln!("fake-redoubt: {e:?}");
            1
        }
    }
}
