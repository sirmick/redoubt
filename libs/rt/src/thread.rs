//! Threads of this process (userland/native.md, "redoubt-rt"): [`spawn`] runs a closure on a new
//! thread, on a stack of its own from `map_anon`.

use alloc::boxed::Box;

use crate::abi::Error;
use crate::handle::{thread_create, thread_exit};
use crate::ipc::Buffer;

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
