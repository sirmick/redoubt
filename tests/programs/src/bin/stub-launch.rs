//! The real loader stub (WP-R2), launched exactly as PACKAGES.md's "Launching a process"
//! describes: a flat binary mapped at a fixed address, a copied ELF image, a startup page naming
//! it. A user parent (this program) launches a well-formed child (`fixture-child`,
//! `stub/src/bin/fixture-child.rs`, which exits 77 from its own entry) and then hostile images
//! built by patching that child's headers. Each hostile child must exit with one of the stub's
//! own codes or fault. Nothing else may be affected: this parent keeps running, the children's
//! budget is empty again after every child, and a well-formed child still runs at the end
//! (WP-R2 acceptance; `tests/stub-launch.toml`).
#![no_std]
#![no_main]

use core::fmt::Write;
use core::mem::size_of;

use redoubt_rt::startup::StartupBuilder;
use stub::{MAX_IMAGE_LEN, STUB_ENTRY};
use test_programs::rd::{self, Cause, ExitNotice, Received, Usage};
use uart_16550::MmioSerialPort;

/// The stub's own flat binary (objcopied by `build.rs`).
static STUB_BIN: &[u8] = include_bytes!(env!("STUB_BIN"));
/// A well-formed ELF the stub should map and jump to.
static CHILD_ELF: &[u8] = include_bytes!(env!("STUB_CHILD_ELF"));

/// Where this launcher puts the copied program image in the child: page-aligned and outside the
/// program link range (`0x1_0000..STUB_ENTRY`, MEMORY-LAYOUT.md).
const IMAGE_AT: usize = 0x4000_0000;
const STACK_PAGES: usize = 8;
const WAIT: u64 = 2_000_000;

/// The "huge image_len" case's length: over the stub's cap, yet `IMAGE_AT + len` does not
/// overflow, so the startup block itself is well-formed.
const HUGE_LEN: usize = usize::MAX - IMAGE_AT;
const _: () = assert!(HUGE_LEN > MAX_IMAGE_LEN);

/// `fixture-child`'s own exit code (`stub/src/bin/fixture-child.rs`).
const CHILD_OK: u32 = 77;
/// The stub's exit codes (`stub/src/main.rs`, `mod exit`).
const BAD_STARTUP: u32 = 110;
const BAD_IMAGE: u32 = 111;

/// Where the stack and the startup page go in the child.
#[derive(Clone, Copy)]
struct Layout {
    stack_top: usize,
    startup_at: usize,
}

/// The launch convention: both outside the program link range, so no honest segment meets them.
const OUTSIDE: Layout = Layout { stack_top: 0x8000_0000, startup_at: 0x7FF0_0000 };
/// Both inside the link range, so a hostile segment can name them: the startup page is caught by
/// the stub's own `exclude` check, the stack only by the kernel's `map_fixed` overlap check.
const INSIDE: Layout = Layout { stack_top: 0x1000_0000, startup_at: 0x0F00_0000 };

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

/// What a case accepts.
#[derive(Clone, Copy)]
enum Expect {
    /// `Exited` with exactly this code.
    Code(u32),
    /// `Faulted`, any code.
    Fault,
}

#[no_mangle]
pub extern "C" fn _start(_arg: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    say!(out, "\n[stub-launch] starting");
    if CHILD_ELF.len() > MAX_CHILD_ELF {
        say!(out, "[stub-launch] FAIL: fixture-child is {} bytes, over MAX_CHILD_ELF", CHILD_ELF.len());
        rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
        test_programs::park()
    }

    // Every child runs in its own users' budget, which must be back to empty after each one:
    // whatever a child mapped (the stub, its image, its segments) is freed with it.
    let budget = rd::create(rd::USERS, &rd::spec(600, 1, 100)).expect("a budget for the children");
    let kids = &Kids { budget, empty: rd::usage(budget).expect("usage") };
    let mut ok = check(
        &mut out,
        kids,
        "well-formed child",
        CHILD_ELF,
        CHILD_ELF.len(),
        OUTSIDE,
        Expect::Code(CHILD_OK),
    );

    let mut buf = [0u8; MAX_CHILD_ELF];
    let child = Elf::new(&mut buf);
    let n = child.len;
    let (ro, rx) = child.loads();
    let ro_vaddr = child.ph(ro, Ph::Vaddr);

    let mut hostile =
        |out: &mut Out, name: &str, len: usize, layout: Layout, expect: Expect, patch: &dyn Fn(&mut Elf)| {
            let mut buf = [0u8; MAX_CHILD_ELF];
            let mut elf = Elf::new(&mut buf);
            patch(&mut elf);
            ok &= check(out, kids, name, &elf.bytes[..n], len, layout, expect);
        };

    // The code segment moved onto the read-only segment's page (same offset within the page, so
    // `p_align` still holds): two segments overlap.
    hostile(&mut out, "overlapping segments", n, OUTSIDE, Expect::Code(BAD_IMAGE), &|e| {
        let at = ro_vaddr + e.ph(rx, Ph::Vaddr) % rd::PAGE_SIZE;
        e.set_ph(rx, Ph::Vaddr, at);
    });
    // A segment straddling the stub's first page.
    hostile(&mut out, "segment over the stub", n, OUTSIDE, Expect::Code(BAD_IMAGE), &|e| {
        e.set_ph(ro, Ph::Vaddr, STUB_ENTRY - rd::PAGE_SIZE);
        e.set_ph(ro, Ph::Memsz, 2 * rd::PAGE_SIZE);
    });
    hostile(&mut out, "segment over the startup page", n, INSIDE, Expect::Code(BAD_IMAGE), &|e| {
        e.set_ph(ro, Ph::Vaddr, INSIDE.startup_at);
    });
    // The stub cannot see the stack; the kernel refuses the overlap (`InvalidArgument`), which
    // the stub reports as a bad image (round-3 red note 1).
    hostile(&mut out, "segment over the stack", n, INSIDE, Expect::Code(BAD_IMAGE), &|e| {
        e.set_ph(ro, Ph::Vaddr, INSIDE.stack_top - rd::PAGE_SIZE);
    });
    hostile(&mut out, "entry in a read-only segment", n, OUTSIDE, Expect::Code(BAD_IMAGE), &|e| {
        e.set_entry(ro_vaddr);
    });
    hostile(&mut out, "entry outside every segment", n, OUTSIDE, Expect::Code(BAD_IMAGE), &|e| {
        e.set_entry(0x0100_0000);
    });
    // `image_len` cuts the file header, then the program header table, short.
    hostile(&mut out, "truncated file header", 40, OUTSIDE, Expect::Code(BAD_IMAGE), &|_| {});
    let phdrs_cut = child.phoff + child.phentsize + 8;
    hostile(&mut out, "truncated program headers", phdrs_cut, OUTSIDE, Expect::Code(BAD_IMAGE), &|_| {});
    // Over the stub's cap (`stub::MAX_IMAGE_LEN`) without overflowing `image_addr + image_len`.
    hostile(&mut out, "huge image_len", HUGE_LEN, OUTSIDE, Expect::Code(BAD_STARTUP), &|_| {});
    // A segment whose file bytes lie 16 MiB past the one mapped image page, still inside
    // `image_len`: the stub's copy reads unmapped memory and only the child faults.
    hostile(&mut out, "segment bytes past the mapped image", 32 << 20, OUTSIDE, Expect::Fault, &|e| {
        let offset = e.ph(rx, Ph::Offset) + (16 << 20);
        e.set_ph(rx, Ph::Offset, offset);
    });

    // Header fuzz: a fixed seed, so a failure reproduces. Every mutated image must end in a
    // notice (any exit code, or a fault) and leave the children's budget empty.
    let mut seed: u64 = 0x5eed_0f_57ab;
    let header = child.phoff + child.phnum * child.phentsize;
    let (mut exited, mut faulted) = (0, 0);
    for round in 0..FUZZ_ROUNDS {
        let mut buf = [0u8; MAX_CHILD_ELF];
        let elf = Elf::new(&mut buf);
        for _ in 0..1 + next(&mut seed) % 4 {
            let at = next(&mut seed) as usize % header;
            elf.bytes[at] = next(&mut seed) as u8;
        }
        match launch(kids.budget, &elf.bytes[..n], n, OUTSIDE) {
            Ok(Some(notice)) if notice.cause == Cause::Exited => exited += 1,
            Ok(Some(notice)) if notice.cause == Cause::Faulted => faulted += 1,
            Ok(other) => {
                say!(out, "[stub-launch] FAIL: fuzz round {round}: {}", describe(&other));
                ok = false;
            }
            Err((step, e)) => {
                say!(out, "[stub-launch] FAIL: fuzz round {round}: {step}: {e:?}");
                ok = false;
            }
        }
        if rd::usage(kids.budget).expect("usage") != kids.empty {
            say!(out, "[stub-launch] FAIL: fuzz round {round}: the child's budget is not empty");
            ok = false;
        }
    }
    say!(out, "[stub-launch] fuzz: {FUZZ_ROUNDS} images, {exited} exited, {faulted} faulted");

    ok &= check(
        &mut out,
        kids,
        "well-formed child after the attacks",
        CHILD_ELF,
        CHILD_ELF.len(),
        OUTSIDE,
        Expect::Code(CHILD_OK),
    );
    if ok {
        say!(out, "[stub-launch] STUB LAUNCH TEST PASSED");
    }
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

const FUZZ_ROUNDS: u32 = 32;

/// xorshift64: small, deterministic, no state beyond the seed.
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

/// The budget every child runs in, and its usage with nothing in it.
struct Kids {
    budget: u32,
    empty: Usage,
}

/// Launches `image` naming `image_len` bytes, and reports whether it ended as `expect` says and
/// left the children's budget empty again.
fn check(
    out: &mut Out,
    kids: &Kids,
    name: &str,
    image: &[u8],
    image_len: usize,
    layout: Layout,
    expect: Expect,
) -> bool {
    let notice = match launch(kids.budget, image, image_len, layout) {
        Ok(notice) => notice,
        Err((step, e)) => {
            say!(out, "[stub-launch] FAIL: {name}: {step}: {e:?}");
            return false;
        }
    };
    let after = rd::usage(kids.budget).expect("usage");
    if after != kids.empty {
        say!(
            out,
            "[stub-launch] FAIL: {name}: the child's budget is not empty: pages {}, processes {}",
            after.pages_usage,
            after.processes_usage
        );
        return false;
    }
    let passed = match (expect, &notice) {
        (Expect::Code(code), Some(n)) => n.cause == Cause::Exited && n.code == code,
        (Expect::Fault, Some(n)) => n.cause == Cause::Faulted,
        (_, None) => false,
    };
    let code = notice.as_ref().map_or(0, |n| n.code);
    if passed {
        say!(out, "[stub-launch] ok: {name}: {} {code}", describe(&notice));
    } else {
        say!(out, "[stub-launch] FAIL: {name}: {} {code}", describe(&notice));
    }
    passed
}

/// Starts `image` through the real stub, exactly as PACKAGES.md's "Launching a process"
/// describes, with the startup block naming `image_len` bytes, and returns its exit notice.
/// The child runs in `budget`.
fn launch(budget: u32, image: &[u8], image_len: usize, layout: Layout) -> Result<Option<ExitNotice>, Step> {
    let exit = rd::endpoint_create().map_err(at("an exit endpoint"))?;
    let process = rd::process_create(budget, exit).map_err(at("process_create"))?;
    let rw = rd::MemFlags::READ | rd::MemFlags::WRITE;

    // 3. Map the loader stub, a flat binary, read+exec, at its fixed address.
    let stub_pages = STUB_BIN.len().next_multiple_of(rd::PAGE_SIZE);
    let scratch = rd::map_anon(stub_pages, rw).map_err(at("scratch for stub"))?;
    copy_in(scratch, STUB_BIN);
    rd::process_map(process, scratch, STUB_ENTRY, stub_pages, rd::MemFlags::READ | rd::MemFlags::EXECUTE)
        .map_err(at("map the stub"))?;

    // 4. Copy the program's ELF bytes in, read-write, as data.
    let image_pages = image.len().next_multiple_of(rd::PAGE_SIZE);
    let scratch = rd::map_anon(image_pages, rw).map_err(at("scratch for image"))?;
    copy_in(scratch, image);
    rd::process_map(process, scratch, IMAGE_AT, image_pages, rw).map_err(at("map the image"))?;

    // A stack (`process_start`'s `sp`, the stub's own and then the started program's).
    let stack_len = STACK_PAGES * rd::PAGE_SIZE;
    let stack_scratch = rd::map_anon(stack_len, rw).map_err(at("scratch for stack"))?;
    rd::process_map(process, stack_scratch, layout.stack_top - stack_len, stack_len, rw)
        .map_err(at("map the stack"))?;

    // The startup page, naming the image (INIT.md, Startup block; PACKAGES.md step 4).
    let block = StartupBuilder::new(0)
        .image(IMAGE_AT, image_len)
        .finish()
        .map_err(|_| ("a startup block", rd::Error::InvalidArgument))?;
    let page_scratch = rd::map_anon(rd::PAGE_SIZE, rw).map_err(at("scratch for startup"))?;
    copy_in(page_scratch, &block);
    rd::process_map(process, page_scratch, layout.startup_at, rd::PAGE_SIZE, rd::MemFlags::READ)
        .map_err(at("map the startup page"))?;

    rd::process_start(process, STUB_ENTRY, layout.stack_top - 16, layout.startup_at, &[])
        .map_err(at("process_start"))?;

    let notice = match rd::receive(Some(exit), WAIT, 0) {
        Ok(Received::Exit(notice)) => Some(notice),
        _ => None,
    };
    // Receiving the notice freed the process object and its handle (KERNEL-SPEC.md, Process).
    rd::close(exit).map_err(at("close the exit endpoint"))?;
    Ok(notice)
}

/// A launch step that failed, and how: the parent's own calls, never the child's doing.
type Step = (&'static str, rd::Error);

fn at(step: &'static str) -> impl Fn(rd::Error) -> Step { move |e| (step, e) }

fn copy_in(dst: usize, src: &[u8]) {
    for (i, byte) in src.iter().enumerate() {
        // SAFETY: `dst..dst+src.len()` is memory this process just mapped read-write via
        // `map_anon`; `i < src.len()`.
        unsafe { (dst as *mut u8).add(i).write_volatile(*byte) };
    }
}

fn describe(notice: &Option<ExitNotice>) -> &'static str {
    match notice {
        None => "no notice",
        Some(n) if n.cause == Cause::Exited => "exited",
        Some(n) if n.cause == Cause::Faulted => "faulted",
        Some(_) => "other",
    }
}

/// An upper bound on `fixture-child`'s size, for the mutable copies this bin patches (no `alloc`
/// here, unlike the stub crate).
const MAX_CHILD_ELF: usize = 4096;

/// A program header field.
#[derive(Clone, Copy)]
enum Ph {
    Type,
    Offset,
    Vaddr,
    Memsz,
}

/// A copy of `fixture-child` whose header fields can be patched, at this target's own ELF class
/// (the stub refuses the other one).
struct Elf<'a> {
    bytes: &'a mut [u8],
    len: usize,
    phoff: usize,
    phentsize: usize,
    phnum: usize,
}

impl<'a> Elf<'a> {
    #[cfg(target_pointer_width = "64")]
    const E_ENTRY: usize = 24;
    #[cfg(target_pointer_width = "32")]
    const E_ENTRY: usize = 24;
    #[cfg(target_pointer_width = "64")]
    const E_PHENTSIZE: usize = 54;
    #[cfg(target_pointer_width = "32")]
    const E_PHENTSIZE: usize = 42;
    #[cfg(target_pointer_width = "64")]
    const E_PHOFF: usize = 32;
    #[cfg(target_pointer_width = "32")]
    const E_PHOFF: usize = 28;

    fn new(buf: &'a mut [u8; MAX_CHILD_ELF]) -> Elf<'a> {
        buf[..CHILD_ELF.len()].copy_from_slice(CHILD_ELF);
        let mut elf = Elf { bytes: buf, len: CHILD_ELF.len(), phoff: 0, phentsize: 0, phnum: 0 };
        elf.phoff = elf.word(Self::E_PHOFF);
        elf.phentsize = elf.half(Self::E_PHENTSIZE);
        elf.phnum = elf.half(Self::E_PHENTSIZE + 2);
        elf
    }

    fn half(&self, at: usize) -> usize { u16::from_le_bytes([self.bytes[at], self.bytes[at + 1]]) as usize }

    fn word(&self, at: usize) -> usize {
        let mut raw = [0u8; size_of::<usize>()];
        raw.copy_from_slice(&self.bytes[at..at + size_of::<usize>()]);
        usize::from_le_bytes(raw)
    }

    fn set_word(&mut self, at: usize, value: usize) {
        self.bytes[at..at + size_of::<usize>()].copy_from_slice(&value.to_le_bytes());
    }

    /// Where field `field` of program header `i` starts. `p_type` is 4 bytes on both classes;
    /// the others are address-sized.
    #[cfg(target_pointer_width = "64")]
    fn field(&self, i: usize, field: Ph) -> usize {
        let base = self.phoff + i * self.phentsize;
        base + match field {
            Ph::Type => 0,
            Ph::Offset => 8,
            Ph::Vaddr => 16,
            Ph::Memsz => 40,
        }
    }

    #[cfg(target_pointer_width = "32")]
    fn field(&self, i: usize, field: Ph) -> usize {
        let base = self.phoff + i * self.phentsize;
        base + match field {
            Ph::Type => 0,
            Ph::Offset => 4,
            Ph::Vaddr => 8,
            Ph::Memsz => 20,
        }
    }

    fn ph(&self, i: usize, field: Ph) -> usize {
        match field {
            Ph::Type => {
                let at = self.field(i, field);
                u32::from_le_bytes([
                    self.bytes[at],
                    self.bytes[at + 1],
                    self.bytes[at + 2],
                    self.bytes[at + 3],
                ]) as usize
            }
            _ => self.word(self.field(i, field)),
        }
    }

    fn set_ph(&mut self, i: usize, field: Ph, value: usize) { self.set_word(self.field(i, field), value) }

    fn set_entry(&mut self, value: usize) { self.set_word(Self::E_ENTRY, value) }

    /// The indices of the read-only and the code `PT_LOAD` headers, in that order: the linker
    /// emits `.rodata` first, then `.text` (`readelf -l`).
    fn loads(&self) -> (usize, usize) {
        let mut loads = (0..self.phnum).filter(|&i| self.ph(i, Ph::Type) == 1);
        let ro = loads.next().expect("a read-only PT_LOAD");
        let rx = loads.next().expect("a code PT_LOAD");
        (ro, rx)
    }
}
