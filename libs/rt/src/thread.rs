//! Threads of this process (userland/native.md, "redoubt-rt"): [`spawn`] runs a closure on a new
//! thread, on a stack of its own from `map_anon`; [`scope`] runs threads that borrow from their
//! caller and ends them all before it returns.

use alloc::boxed::Box;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::abi::Error;
use crate::handle::{thread_create, thread_exit};
use crate::ipc::Buffer;
use crate::sync::Semaphore;

/// What a new thread runs. `spawn` boxes it once more, so the trampoline's one argument register
/// holds a thin pointer to it.
type Body = Box<dyn FnOnce() + Send + 'static>;

/// Runs `body` on a new thread of this process, on a stack of `stack_pages` pages from
/// `map_anon`, and returns the kernel's thread id. The thread exits when `body` returns.
///
/// - **The stack is not freed** when the thread ends: [`thread_create`] takes its pages for good, so no owner
///   can hand them out underneath a thread still on them. A program that starts threads over and over spends
///   their stacks; one that starts a few that live as long as it does, as beamlet's platform does, spends
///   them once.
/// - **No guard page below it:** a stack that overflows writes over whatever is mapped below. The caller
///   sizes the stack for the deepest call the closure makes.
/// - **A panic on the thread ends the process**, as any panic does ([`crate::start`]).
///
/// If the kernel refuses the thread, `body` is dropped without running, with its stack. A start
/// whose answer cannot be read (`InvalidArgument`, which a refusal of a bad argument cannot be
/// told from) may have started the thread, so `body` is then never freed, and [`thread_create`]
/// keeps its stack: both leak rather than being freed under a thread that may be running on them.
pub fn spawn(body: Body, stack_pages: usize) -> Result<u32, Error> {
    let stack = Buffer::new(stack_pages)?;
    let arg = Box::into_raw(Box::new(body)) as usize;
    thread_create(trampoline, stack, arg).inspect_err(|e| {
        if *e != Error::InvalidArgument {
            drop(take(arg));
        }
    })
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
