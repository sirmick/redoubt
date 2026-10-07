// SPDX-License-Identifier: MIT OR Apache-2.0

//! The harts: each one's block, the start of the others, and the reschedule interrupt
//! (docs/plan/m2-usable-shell.md, "Several harts"; kernel/memory-layout.md, "Per-process kernel
//! data").
//!
//! Per-hart state is indexed by a dense boot index below [`MAX_HARTS`], never by the hart id,
//! which a platform may number sparsely and widely. The boot hart is index 0; the loader lists the
//! others it found in the device tree, in tree order, with the ones the kernel cannot run already
//! left out (`Hart` in kernel/boot.md, "The argument block").
//!
//! A hart's block is kernel data, mapped in every address space, and `sscratch` holds its address
//! whenever the hart is not in the first instructions of the trap entry (`asm.rs`), which finds the
//! running thread's context through it. Only its own hart writes a block, but for its `idle`
//! flag, which a hart holding the kernel lock clears on another as it wakes it ([`wake_idle`]),
//! and its shootdown request, which one sets on another ([`shootdown`]).

use core::sync::atomic::{AtomicUsize, Ordering};

pub use redoubt_layout::MAX_HARTS;
use redoubt_layout::{KERNEL_PID, Pid};

use super::process::{INITIAL_TID, Thread};

/// One hart's kernel state. `repr(C)`: the trap entry and the start trampoline (`asm.rs`) read it
/// by word offset, the `BLOCK_*` constants. Aligned to a power of two above its size, so that
/// [`index`], which every per-hart read and the kernel lock's checked-build holder test make,
/// is a shift and not a division.
#[cfg_attr(target_pointer_width = "64", repr(C, align(512)))]
#[cfg_attr(target_pointer_width = "32", repr(C, align(256)))]
pub struct Block {
    /// Where the trap entry stashes `x1` while it finds the context.
    scratch: AtomicUsize,
    /// The address of the running thread's saved context, where the trap entry saves its
    /// registers.
    context: AtomicUsize,
    /// The top of this hart's trap stack.
    trap_sp: AtomicUsize,
    /// The PID this hart runs (0 until its first switch: the kernel).
    pid: AtomicUsize,
    /// The thread it runs, 0 for none.
    tid: AtomicUsize,
    /// The hart id the firmware knows it by.
    id: AtomicUsize,
    /// What the start trampoline loads, with the MMU off: the kernel's `satp`, the top of the
    /// hart's kernel stack, the virtual address it lands at, and this block's virtual address.
    start_satp: AtomicUsize,
    start_sp: AtomicUsize,
    start_entry: AtomicUsize,
    start_block: AtomicUsize,
    /// Set while the hart idles in `kmain`, so a wake sends it the reschedule interrupt.
    idle: AtomicUsize,
    /// A shootdown asked of this hart: the ASID to flush, plus one; 0 for none ([`shootdown`]).
    shoot: AtomicUsize,
    /// The PID whose space this hart left at its last shootdown, until it next switches process
    /// ([`audit_left`]).
    left: AtomicUsize,
    /// The hart's own saved context for when it runs the kernel's thread (PID 1, which has no
    /// budget and no IPC pages): one `Thread`, reached by address as every context is.
    kernel: [AtomicUsize; 32],
}

/// Word offsets the assembly uses.
pub const BLOCK_SCRATCH: usize = 0;
pub const BLOCK_CONTEXT: usize = 1;
pub const BLOCK_TRAP_SP: usize = 2;
pub const BLOCK_START_SATP: usize = 6;
pub const BLOCK_START_SP: usize = 7;
pub const BLOCK_START_ENTRY: usize = 8;
pub const BLOCK_START_BLOCK: usize = 9;

const WORD: usize = core::mem::size_of::<usize>();
const _: () = {
    assert!(core::mem::offset_of!(Block, scratch) == BLOCK_SCRATCH * WORD);
    assert!(core::mem::offset_of!(Block, context) == BLOCK_CONTEXT * WORD);
    assert!(core::mem::offset_of!(Block, trap_sp) == BLOCK_TRAP_SP * WORD);
    assert!(core::mem::offset_of!(Block, start_satp) == BLOCK_START_SATP * WORD);
    assert!(core::mem::offset_of!(Block, start_sp) == BLOCK_START_SP * WORD);
    assert!(core::mem::offset_of!(Block, start_entry) == BLOCK_START_ENTRY * WORD);
    assert!(core::mem::offset_of!(Block, start_block) == BLOCK_START_BLOCK * WORD);
    assert!(core::mem::size_of::<[AtomicUsize; 32]>() == core::mem::size_of::<Thread>());
    assert!(core::mem::align_of::<Block>() >= core::mem::align_of::<Thread>());
    assert!(core::mem::size_of::<Block>().is_power_of_two());
};

impl Block {
    const fn new() -> Self {
        Block {
            scratch: AtomicUsize::new(0),
            context: AtomicUsize::new(0),
            trap_sp: AtomicUsize::new(0),
            pid: AtomicUsize::new(0),
            tid: AtomicUsize::new(0),
            id: AtomicUsize::new(0),
            start_satp: AtomicUsize::new(0),
            start_sp: AtomicUsize::new(0),
            start_entry: AtomicUsize::new(0),
            start_block: AtomicUsize::new(0),
            idle: AtomicUsize::new(0),
            shoot: AtomicUsize::new(0),
            left: AtomicUsize::new(0),
            kernel: [const { AtomicUsize::new(0) }; 32],
        }
    }

    /// The hart runs the kernel's thread (`kmain`), saving into its own area.
    fn runs_kernel(&self) {
        self.tid.store(INITIAL_TID, Ordering::Relaxed);
        self.context.store(self.kernel.as_ptr() as usize, Ordering::Relaxed);
    }
}

/// Every hart's block, by boot index. `_start` points the boot hart's `sscratch` at the first.
#[no_mangle]
pub static HART_BLOCKS: [Block; MAX_HARTS] = [const { Block::new() }; MAX_HARTS];

/// How many harts run the kernel: the boot hart and those started since.
static STARTED: AtomicUsize = AtomicUsize::new(1);

/// How many harts the device tree has (`Hart`), for the checked build's account.
static FOUND: AtomicUsize = AtomicUsize::new(1);

/// This hart's boot index: which block `sscratch` names.
pub fn index() -> usize {
    riscv::register::sscratch::read().wrapping_sub(HART_BLOCKS.as_ptr() as usize)
        / core::mem::size_of::<Block>()
}

/// This hart's block.
fn this() -> &'static Block { &HART_BLOCKS[index()] }

/// How many harts run the kernel now.
pub fn started() -> usize { STARTED.load(Ordering::Relaxed) }

/// The PID this hart runs (kernel/memory-layout.md, "`satp`").
pub fn pid() -> Pid { Pid::new(this().pid.load(Ordering::Relaxed) as u16).unwrap_or(KERNEL_PID) }

pub fn set_pid(pid: Pid) {
    this().pid.store(usize::from(pid.get()), Ordering::Relaxed);
    this().left.store(0, Ordering::Relaxed);
    #[cfg(debug_assertions)]
    if pid != KERNEL_PID {
        RAN_USER.fetch_or(1 << index(), Ordering::Relaxed);
    }
}

/// A checked build's record of the harts that have run a process, a bit per boot index.
#[cfg(debug_assertions)]
static RAN_USER: AtomicUsize = AtomicUsize::new(0);

/// A checked build's account at `system_reset`, for `smp-boot`: whether every hart in the device
/// tree was started and ran a process, and the kernel lock's FIFO evidence, the most kernel
/// sections any acquisition waited behind (cell.rs; at most the harts less one, R78). A short
/// count is a fact, not a failure: a boot with fewer runnable processes than harts leaves some
/// idle, and only `smp-boot`, whose work fills every hart, judges it.
#[cfg(debug_assertions)]
pub fn report() {
    let (ran, started, found) =
        (RAN_USER.load(Ordering::Relaxed).count_ones() as usize, started(), FOUND.load(Ordering::Relaxed));
    if ran == started && started == found {
        println!("harts: all {} in the tree ran user code", found);
    } else {
        println!("harts: {} in the tree, {} started, {} ran user code", found, started, ran);
    }
    println!(
        "kernel lock: most waited {} section(s), {} hart(s)",
        crate::cell::KERNEL_LOCK.most_waited(),
        started
    );
}

/// The thread this hart runs, 0 for none.
pub fn tid() -> usize { this().tid.load(Ordering::Relaxed) }

/// The address of the running thread's saved context.
pub fn context() -> usize { this().context.load(Ordering::Relaxed) }

/// This hart runs thread `tid`, whose context is at `context`.
pub fn set_thread(tid: usize, context: usize) {
    let block = this();
    block.tid.store(tid, Ordering::Relaxed);
    block.context.store(context, Ordering::Relaxed);
}

/// The trap entry saves into `context` from now on (the running thread was destroyed).
pub fn set_context(context: usize) { this().context.store(context, Ordering::Relaxed) }

/// The address of this hart's own saved context for the kernel's thread.
pub fn kernel_context() -> usize { this().kernel.as_ptr() as usize }

/// The boot hart, first thing in `init`: its trap stack, the loader's. Its id is the first the
/// loader lists ([`start_others`]).
pub fn init_boot() {
    let block = &HART_BLOCKS[0];
    block.trap_sp.store(redoubt_layout::TRAP_STACK_TOP - 16, Ordering::Relaxed);
    block.runs_kernel();
}

/// This hart is about to idle (`true`) or has stopped idling.
pub fn set_idle(idle: bool) { this().idle.store(usize::from(idle), Ordering::Relaxed) }

/// A wake made a budget runnable: send the reschedule interrupt to one idle hart, if any, so it
/// picks. Called holding the kernel lock; the hart takes it once it holds the lock itself.
pub fn wake_idle() {
    let me = index();
    for (i, block) in HART_BLOCKS[..started()].iter().enumerate() {
        if i != me && block.idle.swap(0, Ordering::Relaxed) != 0 {
            let id = block.id.load(Ordering::Relaxed);
            let _ = sbi_rt::send_ipi(sbi_rt::HartMask::from_mask_base(1, id));
            return;
        }
    }
}

/// Shoot `pid` down on every other hart running it, before any of its frames is freed
/// (kernel/memory.md, "Residual risks"): each is asked to flush its ASID and sent the interrupt,
/// and the caller, holding the kernel lock, waits for every acknowledgement. No hart needs the
/// lock to acknowledge ([`serve`]), so there is no deadlock. The set is the harts whose block names
/// `pid` as the process they run, which is exact: it changes only under the lock. SMP1's one
/// caller is a destruction (`ptable.rs`, `kill_process`), and the set has at most one hart, which
/// this does not assume.
pub fn shootdown(pid: Pid) {
    // Debug only, never in a bench build but one recorded negative run: no hart is shot down.
    if cfg!(feature = "smp-no-evict") {
        return;
    }
    let me = index();
    let target = usize::from(pid.get());
    let mut asked = 0usize;
    for (i, block) in HART_BLOCKS[..started()].iter().enumerate() {
        if i != me && block.pid.load(Ordering::Relaxed) == target {
            block.shoot.store(target + 1, Ordering::Release);
            let _ = sbi_rt::send_ipi(sbi_rt::HartMask::from_mask_base(1, block.id.load(Ordering::Relaxed)));
            asked |= 1 << i;
        }
    }
    for (_, block) in HART_BLOCKS.iter().enumerate().filter(|(i, _)| asked & 1 << i != 0) {
        while block.shoot.load(Ordering::Acquire) != 0 {
            crate::cell::pause();
        }
    }
    // The checked build's word for `smp-evict`: every hart running it has left it.
    #[cfg(debug_assertions)]
    if asked != 0 {
        println!(
            "shootdown: PID {} stopped on hart(s) {:#b} before any of its frames is freed",
            target, asked
        );
    }
}

/// Serve this hart's shootdown request, if there is one: at a trap from user mode before the
/// kernel lock is taken, and on every turn of the wait for it. The hart leaves for the kernel's
/// own space, flushes the ASID, fences its instruction fetches and acknowledges; it reads no
/// kernel cell. Its block still names the process, which the kernel finds dead once the hart
/// holds the lock (`irq.rs`).
pub fn serve() {
    let block = this();
    let asked = block.shoot.load(Ordering::Acquire);
    if asked != 0 {
        super::mem::shot_down(asked - 1);
        block.left.store(asked - 1, Ordering::Relaxed);
        // Served by polling or by its interrupt: either way the interrupt has done its work, and a
        // hart running a process is never idle, so no reschedule interrupt is pending to lose.
        ack_ipi();
        block.shoot.store(0, Ordering::Release);
    }
}

/// Whether this hart's process was shot down since it was switched to: its block names the PID
/// whose space it left ([`serve`]), until the next switch clears it ([`set_pid`]).
pub fn shot_down() -> bool {
    let block = this();
    let left = block.left.load(Ordering::Relaxed);
    left != 0 && left == block.pid.load(Ordering::Relaxed)
}

/// The checked build's audit before a destruction frees anything of `pid` (`ptable.rs`): every
/// other hart whose block names `pid` has left its space, so no hart's walker or cached translation
/// can reach a frame it frees. It reads the harts' blocks, not the shootdown's own record, so a
/// destruction that shoots nothing down (`smp-no-evict`) trips it.
#[cfg(debug_assertions)]
pub fn audit_left(pid: Pid) {
    let (me, target) = (index(), usize::from(pid.get()));
    for (i, block) in HART_BLOCKS[..started()].iter().enumerate() {
        let runs = block.pid.load(Ordering::Relaxed) == target;
        assert!(
            i == me || !runs || block.left.load(Ordering::Acquire) == target,
            "ASID audit: hart {} still runs PID {} as its frames are about to be freed",
            i,
            target
        );
    }
}

/// The reschedule interrupt was taken: clear it (`sip.SSIP`).
pub fn ack_ipi() {
    // SAFETY: clearing this hart's own pending supervisor software interrupt has no memory effect.
    unsafe { riscv::register::sip::clear_ssoft() };
}

extern "C" {
    fn _hart_start();
    fn _hart_land();
}

/// The `Hart` argument (kernel/boot.md): the harts in the tree, and the listed harts' ids by boot
/// index, the boot hart first. `None` without one: the boot hart runs alone.
fn listed() -> Option<(usize, impl Iterator<Item = usize>)> {
    let arg = crate::args::KernelArguments::get().iter().find(|a| a.name == u32::from_le_bytes(*b"Hart"))?;
    // Words: the harts in the tree, the harts listed; then each listed hart's id (2 words).
    let (found, listed) = (arg.data[0] as usize, arg.data[1] as usize);
    assert!(
        listed >= 1 && listed <= MAX_HARTS && arg.data.len() >= 2 + 2 * listed,
        "a malformed Hart argument"
    );
    Some((found, (0..listed).map(move |i| crate::args::wide(arg.data, 2 + 2 * i))))
}

/// Back the stack slots of the harts the loader listed, but the boot hart's, which the loader
/// mapped (`HART_STACKS`, kernel/memory-layout.md): frames the kernel keeps, taken at boot before
/// `boot_budgets` counts what is left. The guard pages stay unmapped.
pub fn map_stacks(mm: &mut crate::mem::MemoryManager) {
    let Some((_, ids)) = listed() else { return };
    for index in 1..ids.count() {
        let stacks = [
            (redoubt_layout::hart_kernel_stack_top(index), redoubt_layout::KERNEL_STACK_PAGES),
            (redoubt_layout::hart_trap_stack_top(index), redoubt_layout::TRAP_STACK_PAGES),
        ];
        for (top, pages) in stacks {
            for page in 1..=pages {
                let frame = mm.kernel_frame().expect("boot: no RAM for a hart's stacks");
                super::mem::map_kernel_page(frame, top - page * redoubt_sys::PAGE_SIZE);
            }
        }
    }
}

/// Start the harts the loader listed (`Hart`), each at the trampoline with its block: it lands in
/// [`hart_main`]. A hart that does not start is reported, and the boot goes on without it.
pub fn start_others() {
    let Some((found, ids)) = listed() else { return };
    FOUND.store(found, Ordering::Relaxed);
    let satp = super::mem::kernel_satp();
    let tramp = phys_of(_hart_start as *const () as usize);
    let mut n = 0;
    for (i, id) in ids.enumerate() {
        n += 1;
        let block = &HART_BLOCKS[i];
        block.id.store(id, Ordering::Relaxed);
        if i == 0 {
            continue;
        }
        // `map_stacks` backed the slot's two stacks.
        block.trap_sp.store(redoubt_layout::hart_trap_stack_top(i) - 16, Ordering::Relaxed);
        block.runs_kernel();
        block.start_satp.store(satp, Ordering::Relaxed);
        block.start_sp.store(redoubt_layout::hart_kernel_stack_top(i) - 16, Ordering::Relaxed);
        block.start_entry.store(_hart_land as *const () as usize, Ordering::Relaxed);
        block.start_block.store(block as *const Block as usize, Ordering::Relaxed);
        STARTED.store(i + 1, Ordering::Release);
        // The block is published before the hart starts: `hart_start` is an `ecall`, and the
        // fence orders the stores ahead of it.
        core::sync::atomic::fence(Ordering::SeqCst);
        if sbi_rt::hart_start(id, tramp, phys_of(block as *const Block as usize)).is_err() {
            STARTED.store(i, Ordering::Release);
            println!("harts: hart {} did not start", id);
            return;
        }
    }
    if found > n {
        println!(
            "harts: {} of {} parked: past {} or with no PLIC context in reach",
            found - n,
            found,
            MAX_HARTS
        );
    }
}

/// The physical address of kernel address `virt`.
fn phys_of(virt: usize) -> usize {
    let page = super::mem::virt_to_phys(virt).expect("a kernel address has a frame");
    page + (virt & (redoubt_sys::PAGE_SIZE - 1))
}

/// Where a started hart lands, through `_hart_land` (virtual, MMU on, interrupts off, `sp` its
/// kernel stack, `sscratch` its block).
#[no_mangle]
extern "C" fn hart_main(id: usize) -> ! {
    crate::cell::KERNEL_LOCK.acquire();
    // R11 and R24: `_hart_land` wrote them as `_start` does on the boot hart.
    assert_eq!(riscv::register::senvcfg::read().bits(), 0, "senvcfg is not 0 on hart {} (R11)", id);
    let status = riscv::register::sstatus::read();
    assert!(!status.sum() && !status.mxr(), "sstatus.SUM or MXR is set on hart {} (R24)", id);
    super::irq::timer::init_hart();
    println!("hart {} (boot index {}) runs the scheduler", id, index());
    crate::kmain();
    panic!("kmain returned on hart {}", id)
}
