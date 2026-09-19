//! Hostile input: corrupted `.beam` files must be rejected cleanly or, if they still load, run
//! without panicking the VM. The fixtures are real OTP 28 compiler output from `tests/erlang`.

use std::collections::BTreeMap;

use beamlet_vm::loader::{self, LoadError};
use beamlet_vm::platform::{Platform, PlatformError};
use beamlet_vm::vm::Limits;
use beamlet_vm::{atom::AtomTable, Vm};

const FIXTURES: &[(&str, &[u8])] = &[
    ("binaries", include_bytes!("fixtures/binaries.beam")),
    ("exceptions", include_bytes!("fixtures/exceptions.beam")),
    ("funs", include_bytes!("fixtures/funs.beam")),
    ("maps_test", include_bytes!("fixtures/maps_test.beam")),
    ("processes", include_bytes!("fixtures/processes.beam")),
];

/// A platform with a fake clock and a fixed set of modules.
struct TestPlatform {
    now: u64,
    modules: BTreeMap<String, Vec<u8>>,
}

impl Platform for TestPlatform {
    fn monotonic_us(&mut self) -> u64 {
        self.now += 1;
        self.now
    }
    fn system_time_us(&mut self) -> Option<u64> {
        None
    }
    fn idle(&mut self, deadline: Option<u64>) {
        // Time passes instantly in tests.
        if let Some(d) = deadline {
            self.now = self.now.max(d);
        }
    }
    fn console_write(&mut self, _bytes: &[u8]) {}
    fn random(&mut self, _buf: &mut [u8]) -> Result<(), PlatformError> {
        Err(PlatformError::Unavailable)
    }
    fn load_module(&mut self, module: &str) -> Option<Vec<u8>> {
        self.modules.get(module).cloned()
    }
}

/// xorshift64*: a small deterministic PRNG, so failures reproduce exactly.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn mutate(rng: &mut Rng, input: &[u8]) -> Vec<u8> {
    let mut b = input.to_vec();
    for _ in 0..1 + rng.below(4) {
        if b.is_empty() {
            break;
        }
        let i = rng.below(b.len());
        match rng.below(6) {
            0 => b[i] ^= 1 << rng.below(8),
            1 => b[i] = rng.next() as u8,
            2 => b[i] = [0x00, 0xff, 0x7f, 0x80][rng.below(4)],
            3 => b.truncate(i),
            4 => {
                b.remove(i);
            }
            _ => b.insert(i, rng.next() as u8),
        }
    }
    // Usually repair the container's size field, so corruption reaches the chunk parsers instead
    // of stopping at the first length check.
    if b.len() >= 8 && rng.below(4) != 0 {
        let size = (b.len() - 8) as u32;
        b[4..8].copy_from_slice(&size.to_be_bytes());
    }
    b
}

#[test]
fn fixtures_load() {
    for (name, bytes) in FIXTURES {
        let m = loader::load(bytes, &mut AtomTable::new()).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(m.name.as_str(), *name);
    }
}

#[test]
fn every_truncation_is_rejected() {
    for (name, bytes) in FIXTURES {
        for len in 0..bytes.len() {
            let r = loader::load(&bytes[..len], &mut AtomTable::new());
            assert!(r.is_err(), "{name} truncated to {len} bytes loaded");
        }
    }
}

#[test]
fn wrong_formats_are_named() {
    let mut atoms = AtomTable::new();
    assert_eq!(loader::load(b"", &mut atoms).err(), Some(LoadError::NotBeam));
    assert_eq!(loader::load(b"FOR1\0\0\0\x04BEAM", &mut atoms).err(), Some(LoadError::MissingChunk("AtU8")));
    // A chunk length that runs past the end of the file.
    let mut bad = FIXTURES[0].1.to_vec();
    bad[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(loader::load(&bad, &mut atoms).err(), Some(LoadError::NotBeam));
}

/// Load and, where loading succeeds, run thousands of mutants. Nothing may panic.
#[test]
fn mutants_never_panic() {
    // More rounds for a long soak: BEAMLET_FUZZ_ROUNDS=1000000 cargo test --release ...
    let rounds: usize = std::env::var("BEAMLET_FUZZ_ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(20_000);
    let seed: u64 = std::env::var("BEAMLET_FUZZ_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(0x9E37_79B9_7F4A_7C15);
    let mut rng = Rng(seed);
    let mut loaded = 0;
    for round in 0..rounds {
        let (name, original) = FIXTURES[round % FIXTURES.len()];
        let bytes = mutate(&mut rng, original);
        let mut atoms = AtomTable::new();
        // To chase a hang: BEAMLET_FUZZ_TRACE=/tmp/m.beam leaves the current mutant there.
        if let Some(path) = std::env::var_os("BEAMLET_FUZZ_TRACE") {
            eprintln!("round {round}");
            std::fs::write(path, &bytes).unwrap();
        }
        let Ok(module) = loader::load(&bytes, &mut atoms) else { continue };
        loaded += 1;
        let module_name = module.name.as_str().to_string();
        let mut modules = BTreeMap::new();
        modules.insert(module_name.clone(), bytes.clone());
        // Small limits keep each run fast; the limits themselves are what is being tested.
        let limits = Limits { max_binary_bits: 1 << 20, max_stack_slots: 1 << 16, ..Limits::default() };
        let mut vm = Vm::with_limits(Box::new(TestPlatform { now: 0, modules }), limits);
        if let Ok(pid) = vm.spawn(&module_name, "start", Vec::new()) {
            // A mutant may loop forever; that is fine, as long as it does not crash the VM.
            let _ = vm.run_bounded(pid, 2_000);
        }
        let _ = name;
    }
    let _ = rounds;
    // The mutator should leave a fair share loadable, or this test proves little about the
    // interpreter.
    eprintln!("{loaded} of {rounds} mutants loaded and ran");
    assert!(loaded * 20 > rounds, "only {loaded} of {rounds} mutants loaded");
}

/// A process stuck in a loop of plain jumps (no calls, so no reductions) is still preempted,
/// so other processes keep running. `asm/jumploop.S` is the source of the fixture.
#[test]
fn jump_loops_are_preempted() {
    let mut modules = BTreeMap::new();
    modules.insert("jumploop".to_string(), include_bytes!("fixtures/jumploop.beam").to_vec());
    let mut vm = Vm::new(Box::new(TestPlatform { now: 0, modules }));
    let _spinner = vm.spawn("jumploop", "spin", Vec::new()).unwrap();
    let done = vm.spawn("jumploop", "done", Vec::new()).unwrap();
    let r = vm.run_bounded(done, 100).expect("done/0 ran despite the spinning process");
    assert_eq!(r.unwrap().unwrap().to_string(), "42");
}

/// Found by `mutants_never_panic`: a mutant of `exceptions.beam` that allocated empty frames in a
/// loop grew without bound and made every raise slower. It must now hit the stack limit quickly.
#[test]
fn empty_frames_count_against_the_stack() {
    let mut modules = BTreeMap::new();
    modules.insert("exceptions".to_string(), include_bytes!("fixtures/regress-frames.beam").to_vec());
    let limits = Limits { max_binary_bits: 1 << 20, max_stack_slots: 1 << 20, ..Limits::default() };
    let mut vm = Vm::with_limits(Box::new(TestPlatform { now: 0, modules }), limits);
    let pid = vm.spawn("exceptions", "start", Vec::new()).unwrap();
    let r = vm.run_bounded(pid, 100_000).expect("finished");
    let e = r.unwrap().unwrap_err();
    assert_eq!(e.reason.to_string(), "{system_limit,stack}");
}
