//! Launching a native program through the loader stub (servers/init.md, "Launching through the
//! loader stub"): the caller brings the stub's bytes, the program's (bytes it holds, or a reader
//! the library asks for one batch at a time), a budget it carved, the endpoint the exit notice
//! goes to, and what the child is given; the library makes the calls and writes the startup block
//! with the runtime's `StartupBuilder`.
//!
//! What it refuses is refused before `process_create`, so no half-made process is left: more than
//! `MAX_START_HANDLES` handles, an empty image, a stack outside 1..=MAX_STACK_PAGES,
//! or a block the parser refuses (a bad name or path, one given twice). A kernel refusal after
//! `process_create` leaves a process that never started in the caller's budget, which comes back
//! with the error for the caller to destroy.

use alloc::vec::Vec;

use redoubt_rt::abi::{Error as SysError, ExitNotice, Handle, MAX_START_HANDLES, MemFlags, PAGE_SIZE};
use redoubt_rt::handle::{Budget, Endpoint, Process};
use redoubt_rt::ipc::{Buffer, Event};
use redoubt_rt::server::close_delivery;
use redoubt_rt::startup::StartupBuilder;
use stub::{IMAGE_AT, MAX_STACK_PAGES, STACK_TOP, STARTUP_AT, STUB_ENTRY};

use crate::error::{Error, Refusal};
use crate::grants::Grants;

/// The child's stack unless the launcher says otherwise.
pub const STACK_PAGES: usize = 16;

/// The most pages a launch holds at once: it copies the image and the stack into the child this
/// many pages at a time (servers/init.md, "Launching through the loader stub"), and `init`'s
/// bound counts one batch.
pub const PLACE_PAGES: usize = 64;

/// The high half of each painted stack unit; the low half holds the server tag and unit index.
pub const STACK_PAINT: u32 = 0x5354_414b;

/// Unit `index` is counted from the stack's lowest address, in eight-byte steps.
pub const fn stack_paint(tag: u16, index: u16) -> u64 {
    ((STACK_PAINT as u64) << 32) | ((tag as u64) << 16) | index as u64
}

/// Reads `buf.len()` bytes of a program's image from offset `at` into `buf`.
pub type ReadImage<'a> = &'a mut dyn FnMut(usize, &mut [u8]) -> Result<(), SysError>;

/// Where a program's image comes from: bytes the launcher holds (`init`'s bundle), or a reader it
/// calls for each batch (a file on `/boot`), so that it never holds more than one batch.
enum Image<'a> {
    Bytes(&'a [u8]),
    Read { len: usize, read: ReadImage<'a> },
}

impl Image<'_> {
    fn len(&self) -> usize {
        match self {
            Image::Bytes(b) => b.len(),
            Image::Read { len, .. } => *len,
        }
    }
}

/// One launch, assembled.
pub struct Launch<'a> {
    stub: &'a [u8],
    image: Image<'a>,
    budget: Budget,
    exit: Endpoint,
    stack_pages: usize,
    stack_tag: Option<u16>,
    heap_pages: u32,
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
        Launch::with(stub, Image::Bytes(image), budget, exit)
    }

    /// As [`Launch::new`], with an image of `len` bytes that `read` supplies one batch at a
    /// time, as it is placed: the launcher holds one batch of it, never the whole.
    pub fn streamed(
        stub: &'a [u8],
        len: usize,
        read: ReadImage<'a>,
        budget: Budget,
        exit: Endpoint,
    ) -> Launch<'a> {
        Launch::with(stub, Image::Read { len, read }, budget, exit)
    }

    fn with(stub: &'a [u8], image: Image<'a>, budget: Budget, exit: Endpoint) -> Launch<'a> {
        Launch {
            stub,
            image,
            budget,
            exit,
            stack_pages: STACK_PAGES,
            stack_tag: None,
            heap_pages: 0,
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

    /// Paints this child's first-thread stack with `tag` for the bench's RAM measurement; an
    /// untagged stack is zeroed. The startup block carries the tag, which the child's runtime marks
    /// its heap record with.
    pub fn stack_tag(&mut self, tag: u16) -> &mut Self {
        self.stack_tag = Some(tag);
        self
    }

    /// Caps the child's heap at `pages` pages (servers/init.md, "Heaps"); 0, the default, is no cap.
    pub fn heap_pages(&mut self, pages: u32) -> &mut Self {
        self.heap_pages = pages;
        self
    }

    /// What servers granted the child, released when its exit notice arrives.
    pub fn grants(&mut self, grants: Grants) -> &mut Self {
        self.grants = grants;
        self
    }

    /// Starts the child. Every refusal of its own comes before any kernel call.
    #[allow(clippy::result_large_err)]
    pub fn start(mut self) -> Result<Job, Failed> {
        let (slots, block, stack_at) = match self.plan() {
            Ok(plan) => plan,
            Err(error) => return Err(self.failed(error)),
        };
        let process = match Process::create(&self.budget, &self.exit) {
            Ok(process) => process,
            Err(error) => return Err(self.failed(error.into())),
        };
        let rw = MemFlags::READ | MemFlags::WRITE;
        let rx = MemFlags::READ | MemFlags::EXECUTE;
        let image_pages = self.image.len().max(1).div_ceil(PAGE_SIZE);
        let image = &mut self.image;
        let started = place(&process, &mut from(self.stub), pages_of(self.stub), STUB_ENTRY, rx, None)
            .and_then(|()| match image {
                Image::Bytes(b) => place(&process, &mut from(b), image_pages, IMAGE_AT, rw, None),
                // The reader is asked only for the image's own bytes; the rest of its last page
                // stays zero.
                Image::Read { len, read } => {
                    let len = *len;
                    let mut fill = |at: usize, buf: &mut [u8]| {
                        let n = buf.len().min(len.saturating_sub(at));
                        if n > 0 { read(at, &mut buf[..n]) } else { Ok(()) }
                    };
                    place(&process, &mut fill, image_pages, IMAGE_AT, rw, None)
                }
            })
            .and_then(|()| place(&process, &mut from(&[]), self.stack_pages, stack_at, rw, self.stack_tag))
            .and_then(|()| {
                place(&process, &mut from(&block), pages_of(&block), STARTUP_AT, MemFlags::READ, None)
            })
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
        if self.image.len() == 0 {
            return Err(Refusal::EmptyImage.into());
        }
        if self.stack_pages == 0 || self.stack_pages > MAX_STACK_PAGES {
            return Err(Refusal::StackTooLarge.into());
        }
        let stack_at = STACK_TOP - self.stack_pages * PAGE_SIZE;
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
        block.image(IMAGE_AT, self.image.len()).heap_pages(self.heap_pages).tag(self.stack_tag.unwrap_or(0));
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

/// The pages of `bytes`, at least one.
fn pages_of(bytes: &[u8]) -> usize { bytes.len().max(1).div_ceil(PAGE_SIZE) }

/// A fill from bytes held: what lies past their end stays zero.
fn from(bytes: &[u8]) -> impl FnMut(usize, &mut [u8]) -> Result<(), SysError> + '_ {
    move |at, buf| {
        let src = bytes.get(at..).unwrap_or_default();
        let len = src.len().min(buf.len());
        buf[..len].copy_from_slice(&src[..len]);
        Ok(())
    }
}

/// Fills `pages` pages at `dst` in the child, `fill` writing each batch's bytes from its offset
/// (zero after them; with `stack_tag`, the stack's paint instead), `PLACE_PAGES` at a time: each
/// batch is filled into fresh pages and moved in before the next is made, so the caller never
/// holds more than one batch. A refusal leaves the batches already moved in the child, which has
/// not started, and returns the refused batch's pages to the caller.
fn place(
    process: &Process,
    fill: &mut dyn FnMut(usize, &mut [u8]) -> Result<(), SysError>,
    pages: usize,
    dst: usize,
    flags: MemFlags,
    stack_tag: Option<u16>,
) -> Result<(), SysError> {
    // At least one batch: one of no pages is refused, as a whole one was.
    for done in (0..pages.max(1)).step_by(PLACE_PAGES) {
        let n = (pages - done).min(PLACE_PAGES);
        let mut batch = Buffer::new(n)?;
        fill(done * PAGE_SIZE, &mut batch[..n * PAGE_SIZE])?;
        if let Some(tag) = stack_tag {
            for (i, unit) in batch.chunks_exact_mut(8).enumerate() {
                let index = (done * PAGE_SIZE / 8 + i) as u16;
                unit.copy_from_slice(&stack_paint(tag, index).to_le_bytes());
            }
        }
        process.map(batch, dst + done * PAGE_SIZE, flags)?;
    }
    Ok(())
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
                Event::Send(delivery) => close_delivery(&delivery),
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
