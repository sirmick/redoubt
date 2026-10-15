//! Threads of this process (userland/native.md, "redoubt-rt"): [`spawn`] runs a closure on a new
//! thread, on a stack of its own in the thread-stack area, above an unmapped guard page; [`scope`]
//! runs threads that borrow from their caller and ends them all before it returns.

use alloc::boxed::Box;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::abi::{Error, PAGE_SIZE};
use crate::handle::{thread_create, thread_exit};
use crate::ipc::Buffer;
use crate::sync::Semaphore;

/// What a new thread runs. `spawn` boxes it once more, so the trampoline's one argument register
/// holds a thin pointer to it.
type Body = Box<dyn FnOnce() + Send + 'static>;

/// The thread-stack area (kernel/memory-layout.md, "Regions"): where [`spawn`] places every
/// stack, by `map_fixed`, from `STACKS_BASE` up to the launcher's startup block at `STACKS_END`
/// (the stub crate's `STARTUP_AT`). `map_anon` places nothing here (its area ends at
/// `0x7000_0000`), so a page the runtime leaves unmapped stays unmapped under the process's own
/// `map_anon` calls, and a `map_fixed` of the program's own into the area is the program's doing:
/// the next `spawn` whose slot it took is refused.
pub const STACKS_BASE: usize = 0x7000_0000;
/// See [`STACKS_BASE`].
pub const STACKS_END: usize = 0x7FF0_0000;

/// The next slot's address: slots are taken in order and never given back, as the stacks
/// themselves are spent for good ([`spawn`]).
static NEXT: AtomicUsize = AtomicUsize::new(STACKS_BASE);
/// Spawns so far, which number the threads' painted stacks from 1.
static SPAWNED: AtomicUsize = AtomicUsize::new(0);

/// The high half of each painted unit of a spawned stack; the low half holds the launch tag, the
/// thread's number and the unit's index. A different word from the launcher's first-thread paint
/// (the client library's `STACK_PAINT`), whose units are indexed from the stack's bottom with its
/// size known to the bench; a spawned stack's size is the program's, so its units are indexed
/// from the top, and the bench reads the peak as the deepest overwritten unit.
pub const THREAD_PAINT: u32 = 0x5448_5244;

/// Unit `index` of the stack of the `thread`th spawn (from 1) of the process launched with `tag`
/// (servers/init.md, "The startup block"), counted from the stack's top in eight-byte steps.
pub const fn thread_paint(tag: u8, thread: u8, index: u16) -> u64 {
    ((THREAD_PAINT as u64) << 32) | ((tag as u64) << 24) | ((thread as u64) << 16) | index as u64
}

/// Runs `body` on a new thread of this process, on a stack of `stack_pages` pages, and returns
/// the kernel's thread id. The thread exits when `body` returns.
///
/// - **The stack is not freed** when the thread ends: [`thread_create`] takes its pages for good, so no owner
///   can hand them out underneath a thread still on them. A program that starts threads over and over spends
///   their stacks; one that starts a few that live as long as it does, as beamlet's platform does, spends
///   them once.
/// - **An unmapped guard page below it:** the stack is placed in the thread-stack area ([`STACKS_BASE`]) by
///   `map_fixed`, each stack one page above the last one's top, so the page under its lowest is never mapped,
///   and an overflow faults there, ending the process at once with the kernel's fault line, instead of
///   writing over whatever `map_anon` placed below. The guard costs no frame, one page of the area's 255 MiB
///   of address space; a `spawn` past the area is `OutOfMemory`. A frame larger than a page whose first store
///   lands below the guard is not caught; no stack here has one.
/// - **Painted** before the thread starts, every unit with [`thread_paint`] (the launch tag, the spawn's
///   number from 1 and the unit's index from the top), so the bench's memory scan reports the thread's stack
///   peak beside the first thread's (docs/testbench.md, "The memory budget"). The 256th and later spawns, a
///   launch tag past 255 and a stack past 128 pages are not painted.
/// - **A panic on the thread ends the process**, as any panic does ([`crate::start`]).
///
/// If the kernel refuses the thread, `body` is dropped without running, with its stack (its slot
/// is spent). A start whose answer cannot be read (`InvalidArgument`, which a refusal of a bad
/// argument cannot be told from) may have started the thread, so `body` is then never freed, and
/// [`thread_create`] keeps its stack: both leak rather than being freed under a thread that may be
/// running on them.
pub fn spawn(body: Body, stack_pages: usize) -> Result<u32, Error> {
    let mut stack = place(stack_pages)?;
    paint(&mut stack);
    let arg = Box::into_raw(Box::new(body)) as usize;
    thread_create(trampoline, stack, arg).inspect_err(|e| {
        if *e != Error::InvalidArgument {
            drop(take(arg));
        }
    })
}

/// Takes the next slot of the thread-stack area, `stack_pages + 1` pages, and maps all but its
/// lowest page: the guard. A slot is never given back, so a refused mapping spends it too.
fn place(stack_pages: usize) -> Result<Buffer, Error> {
    if stack_pages == 0 {
        return Err(Error::InvalidArgument);
    }
    let slot =
        stack_pages.checked_add(1).and_then(|pages| pages.checked_mul(PAGE_SIZE)).ok_or(Error::TooLarge)?;
    let guard = NEXT
        .try_update(Ordering::AcqRel, Ordering::Acquire, |next| {
            next.checked_add(slot).filter(|end| *end <= STACKS_END)
        })
        .map_err(|_| Error::OutOfMemory)?;
    Buffer::fixed(guard + PAGE_SIZE, stack_pages)
}

/// The launch tag the heap's record carries ([`crate::start`]): 0 on the host, which has no
/// launcher and no heap of the runtime's.
fn launch_tag() -> u64 {
    #[cfg(target_os = "none")]
    {
        crate::HEAP.record().get(crate::heap::Record::TAG)
    }
    #[cfg(not(target_os = "none"))]
    {
        0
    }
}

/// Paints `stack` for the bench's scan, as the `n`th spawn, unless it or the launch tag is past
/// a byte, or the stack has more units than the index counts (128 pages).
fn paint(stack: &mut Buffer) {
    let n = SPAWNED.fetch_add(1, Ordering::AcqRel) + 1;
    let (Ok(thread), Ok(tag)) = (u8::try_from(n), u8::try_from(launch_tag())) else { return };
    if stack.len() / 8 > 1 << 16 {
        return;
    }
    for (index, unit) in stack.chunks_exact_mut(8).rev().enumerate() {
        unit.copy_from_slice(&thread_paint(tag, thread, index as u16).to_le_bytes());
    }
}

/// Threads that may borrow from the caller of [`scope`], each ended before it returns.
pub struct Scope<'scope, 'env: 'scope> {
    /// Released once by each thread as it ends.
    ended: Semaphore,
    /// The threads that may be running: started, or whose start could not be read.
    started: AtomicUsize,
    stack_pages: usize,
    scope: PhantomData<&'scope mut &'scope ()>,
    env: PhantomData<&'env mut &'env ()>,
}

/// Runs `f`, whose [`Scope::spawn`] starts threads that may borrow what outlives the call, as
/// `std::thread::scope` does, and waits for every one of them to end before returning (also if
/// `f` panics, on the host). A thread is started with [`spawn`], so its stack, of `stack_pages`
/// pages, is spent for good: for threads a program starts once.
pub fn scope<'env, F, T>(stack_pages: usize, f: F) -> T
where
    F: for<'scope> FnOnce(&'scope Scope<'scope, 'env>) -> T,
{
    let scope = Scope {
        ended: Semaphore::new(0),
        started: AtomicUsize::new(0),
        stack_pages,
        scope: PhantomData,
        env: PhantomData,
    };
    /// Waits for the scope's threads, however `f` ends.
    struct Join<'a, 'scope, 'env>(&'a Scope<'scope, 'env>);
    impl Drop for Join<'_, '_, '_> {
        fn drop(&mut self) {
            for _ in 0..self.0.started.load(Ordering::Acquire) {
                self.0.ended.acquire();
            }
        }
    }
    let join = Join(&scope);
    let result = f(&scope);
    drop(join);
    result
}

impl<'scope> Scope<'scope, '_> {
    /// Runs `body` on a new thread, ended before [`scope`] returns. A thread the kernel refused
    /// never ran; one whose start cannot be read ([`spawn`]) is waited for as if it ran, since
    /// it may be running on what it borrowed.
    pub fn spawn<F: FnOnce() + Send + 'scope>(&'scope self, body: F) -> Result<(), Error> {
        /// Said by the thread when its body ends, also by a panic, on the host.
        struct Ended<'a>(&'a Semaphore);
        impl Drop for Ended<'_> {
            fn drop(&mut self) { self.0.release(); }
        }
        let ended = &self.ended;
        let body: Box<dyn FnOnce() + Send + 'scope> = Box::new(move || {
            let _ended = Ended(ended);
            body();
        });
        // SAFETY: only the lifetime changes. Everything `body` borrows outlives `'scope`, and
        // `scope` does not return until each thread counted in `started` has released `ended`,
        // which it does as its last act on what it borrowed. A thread that may have started is
        // counted (below), so none can outlive the borrow.
        let body: Body = unsafe { core::mem::transmute::<Box<dyn FnOnce() + Send + 'scope>, Body>(body) };
        match spawn(body, self.stack_pages) {
            Ok(_) => {
                self.started.fetch_add(1, Ordering::AcqRel);
                Ok(())
            }
            Err(Error::InvalidArgument) => {
                self.started.fetch_add(1, Ordering::AcqRel);
                Err(Error::InvalidArgument)
            }
            Err(e) => Err(e),
        }
    }
}

/// The new thread's entry: runs its body, then exits the thread.
extern "C" fn trampoline(arg: usize) -> ! {
    take(arg)();
    thread_exit()
}

/// The body `spawn` put at `arg`, taken back by the new thread, or by `spawn` when the kernel
/// refused to start it.
fn take(arg: usize) -> Body {
    // SAFETY: `arg` is `spawn`'s own `Box::into_raw` of a `Body`, passed once as the new thread's
    // argument. The trampoline is reached only from that `thread_create`, which starts at most one
    // thread, and takes the box back once. `spawn` takes it back only on a refusal the kernel
    // returned before starting anything; on a result it cannot read it leaves the box alone.
    // So the box is taken at most once, and never freed under a running thread.
    *unsafe { Box::from_raw(arg as *mut Body) }
}
