//! `fake-redoubt`: beamlet on Redoubt's platform, run on a host, on the fake kernel.
//!
//!     fake-redoubt [-pa DIR]... [report_memory] MODULE [FUNCTION]
//!
//! It starts the fake kernel, a console server on this terminal (the fixture's, see
//! `beamlet_redoubt::fixture`), and a session process whose namespace holds `/dev/cons`; in that
//! process it runs a VM on [`beamlet_redoubt::Redoubt`], calling `MODULE:FUNCTION()` (`start` by
//! default), and prints how it ended: `beamlet_redoubt::run`, as `beamlet` does on the machine.
//! The VM reaches the console through the client library and the fake kernel's IPC, as it does on
//! Redoubt, and has no file system and no programs yet. Its modules come from the `-pa`
//! directories, where `beamlet`'s come from `/boot`.

use std::path::PathBuf;
use std::process::ExitCode;

use beamlet_redoubt::fixture::{self, Dirs};
use redoubt_fake_kernel::fake;

/// Eight bytes from this machine's random source.
fn host_seed() -> std::io::Result<u64> {
    use std::io::Read;
    let mut bytes = [0; 8];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn usage() -> ExitCode {
    eprintln!("usage: fake-redoubt [-pa DIR]... [report_memory] MODULE [FUNCTION]");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut dirs = Vec::new();
    let mut positional = Vec::new();
    let mut report_memory = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-pa" => match args.next() {
                Some(dir) => dirs.push(PathBuf::from(dir)),
                None => return usage(),
            },
            beamlet_redoubt::REPORT_MEMORY => {
                report_memory = Some((|| None) as beamlet_vm::memory::HeapPages)
            }
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
    let vm = f.run(pid, move || {
        let startup = fixture::startup(&block);
        beamlet_redoubt::run(&startup, Box::new(Dirs(dirs)), &module, &function, None, report_memory)
    });
    let status = vm.join().unwrap_or(1);
    // Ending the console's endpoint ends its server.
    f.destroy(console.pid, console.endpoint);
    let _ = console.thread.join();
    ExitCode::from(u8::try_from(status).unwrap_or(1))
}
