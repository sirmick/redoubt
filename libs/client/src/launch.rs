//! Launching a native program through the loader stub (servers/init.md, "Launching through the
//! loader stub"): the caller brings the stub's and the program's bytes, a budget it carved, the
//! endpoint the exit notice goes to, and what the child is given; the library makes the calls and
//! writes the startup block with the runtime's `StartupBuilder`.
//!
//! What it refuses is refused before `process_create`, so no half-made process is left: more than
//! `MAX_START_HANDLES` handles, an empty image, a stack larger than the space below `STACK_TOP`,
//! or a block the parser refuses (a bad name or path, one given twice). A kernel refusal after
//! `process_create` leaves a process that never started in the caller's budget, which comes back
//! with the error for the caller to destroy.

use alloc::vec::Vec;

use redoubt_rt::abi::{Error as SysError, ExitNotice, Handle, MAX_START_HANDLES, MemFlags, PAGE_SIZE};
use redoubt_rt::handle::{Budget, Endpoint, Process};
use redoubt_rt::ipc::{Buffer, Event};
use redoubt_rt::startup::StartupBuilder;
use stub::{IMAGE_AT, STACK_TOP, STARTUP_AT, STUB_ENTRY};

use crate::error::{Error, Refusal};
use crate::grants::Grants;

/// The child's stack unless the launcher says otherwise.
const STACK_PAGES: usize = 16;

/// One launch, assembled.
pub struct Launch<'a> {
    stub: &'a [u8],
    image: &'a [u8],
    budget: Budget,
    exit: Endpoint,
    stack_pages: usize,
    namespace: Vec<(&'a str, Handle)>,
    handles: Vec<(&'a str, Handle)>,
    args: Vec<&'a str>,
    grants: Grants,
}

/// A launch the kernel refused midway: why, and what the caller gave back to it. The budget holds
/// the process that never started; destroying it ends that.
pub struct Failed {
    pub error: Error,
    pub budget: Budget,
    pub exit: Endpoint,
    pub grants: Grants,
}

impl<'a> Launch<'a> {
    /// `stub` and `image` are bytes the caller read; `budget` is one it carved for the child;
    /// `exit` is a receive right, the job's own, where the child's one exit notice arrives.
    pub fn new(stub: &'a [u8], image: &'a [u8], budget: Budget, exit: Endpoint) -> Launch<'a> {
        Launch {
            stub,
            image,
            budget,
            exit,
            stack_pages: STACK_PAGES,
            namespace: Vec::new(),
            handles: Vec::new(),
            args: Vec::new(),
            grants: Grants::new(),
        }
    }

    /// Binds `handle` at `path` in the child's namespace.
    pub fn namespace(&mut self, path: &'a str, handle: Handle) -> &mut Self {
        self.namespace.push((path, handle));
        self
    }

    /// Gives the child `handle` under `name`.
    pub fn handle(&mut self, name: &'a str, handle: Handle) -> &mut Self {
        self.handles.push((name, handle));
        self
    }

    pub fn arg(&mut self, arg: &'a str) -> &mut Self {
        self.args.push(arg);
        self
    }

    pub fn stack_pages(&mut self, pages: usize) -> &mut Self {
        self.stack_pages = pages;
        self
    }

    /// What servers granted the child, released when its exit notice arrives.
    pub fn grants(&mut self, grants: Grants) -> &mut Self {
        self.grants = grants;
        self
    }

    /// Starts the child. Every refusal of its own comes before any kernel call.
    #[allow(clippy::result_large_err)]
    pub fn start(self) -> Result<Job, Failed> {
        let (slots, block, stack_at) = match self.plan() {
            Ok(plan) => plan,
            Err(error) => return Err(self.failed(error)),
        };
        let process = match Process::create(&self.budget, &self.exit) {
            Ok(process) => process,
            Err(error) => return Err(self.failed(error.into())),
        };
        let rw = MemFlags::READ | MemFlags::WRITE;
        let started = place(&process, self.stub, STUB_ENTRY, MemFlags::READ | MemFlags::EXECUTE)
            .and_then(|()| place(&process, self.image, IMAGE_AT, rw))
            .and_then(|()| process.map(Buffer::new(self.stack_pages)?, stack_at, rw))
            .and_then(|()| place(&process, &block, STARTUP_AT, MemFlags::READ))
            .and_then(|()| process.start(STUB_ENTRY, STACK_TOP - 16, STARTUP_AT, &slots));
        match started {
            Ok(()) => Ok(Job { process, budget: self.budget, exit: self.exit, grants: self.grants }),
            Err(error) => {
                let _ = process.close();
                Err(self.failed(error.into()))
            }
        }
    }

    /// The handles for the child's slots 1..=n, each once, its startup block, and where its stack
    /// goes.
    fn plan(&self) -> Result<(Vec<Handle>, Vec<u8>, usize), Error> {
        if self.image.is_empty() {
            return Err(Refusal::EmptyImage.into());
        }
        let stack_at = self
            .stack_pages
            .checked_mul(PAGE_SIZE)
            .and_then(|stack| STACK_TOP.checked_sub(stack))
            .ok_or(Refusal::StackTooLarge)?;
        let mut slots: Vec<Handle> = Vec::new();
        let mut slot = |handle: Handle| match slots.iter().position(|h| *h == handle) {
            Some(i) => i,
            None => {
                slots.push(handle);
                slots.len() - 1
            }
        };
        let namespace: Vec<_> = self.namespace.iter().map(|(path, h)| (*path, slot(*h))).collect();
        let named: Vec<_> = self.handles.iter().map(|(name, h)| (*name, slot(*h))).collect();
        if slots.len() > MAX_START_HANDLES {
            return Err(Refusal::TooManyHandles.into());
        }
        // The child's handle for slot i is i + 1 (`process_start`).
        let child = |i: usize| Handle::new(i as u32 + 1).expect("a slot is at least 1");
        let mut block = StartupBuilder::new(slots.len() as u32);
        block.image(IMAGE_AT, self.image.len());
        for (path, i) in namespace {
            block.namespace(path, child(i));
        }
        for (name, i) in named {
            block.handle(name, child(i));
        }
        for arg in &self.args {
            block.arg(arg);
        }
        let block = block.finish().map_err(Refusal::Startup)?;
        Ok((slots, block, stack_at))
    }

    fn failed(self, error: Error) -> Failed {
        Failed { error, budget: self.budget, exit: self.exit, grants: self.grants }
    }
}

/// Copies `bytes` into fresh pages and moves them into the child at `dst`.
fn place(process: &Process, bytes: &[u8], dst: usize, flags: MemFlags) -> Result<(), SysError> {
    let mut pages = Buffer::new(bytes.len().max(1).div_ceil(PAGE_SIZE))?;
    pages[..bytes.len()].copy_from_slice(bytes);
    process.map(pages, dst, flags)
}

/// A started child: its exit notice arrives once, on the job's endpoint, and destroying its
/// budget ends it.
pub struct Job {
    process: Process,
    budget: Budget,
    exit: Endpoint,
    grants: Grants,
}

/// How a child ended, and whether its grants were all released.
pub struct Ended {
    pub notice: ExitNotice,
    pub released: Result<(), Error>,
}

impl Job {
    /// Waits `timeout` µs (`FOREVER` for no limit) for the exit notice; when it arrives every
    /// grant made for the child is released and disconnected. Anything else that arrives on the
    /// job's endpoint is refused.
    pub fn wait(&mut self, timeout: u64) -> Result<Ended, Error> {
        loop {
            match self.exit.receive(timeout, 0)? {
                Event::Exit(notice) => return Ok(Ended { notice, released: self.grants.release_all() }),
                // Refused on drop: the handles it carried are closed and it is answered malformed.
                Event::Call(request) => drop(request),
                Event::Send(delivery) => {
                    for handle in delivery.handles.as_slice().iter().flatten() {
                        let _ = redoubt_rt::handle::close(*handle);
                    }
                }
                Event::Interrupt | Event::Abandoned(_) => {}
            }
        }
    }

    /// Ends the child by destroying its budget (kernel/budgets.md R10); its exit notice, `killed`,
    /// then arrives for [`Job::wait`].
    pub fn kill(&self) -> Result<(), Error> { Ok(Budget::from_handle(self.budget.handle()).destroy()?) }

    pub fn process(&self) -> &Process { &self.process }

    pub fn budget(&self) -> &Budget { &self.budget }
}
