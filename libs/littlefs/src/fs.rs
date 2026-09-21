//! The filesystem: device access, metadata pairs on disk, the block allocator, commits, and
//! mount/format with the repairs a mount may need.
//!
//! Structure follows the C reference (`lfs.c`), simplified where the reference carries
//! features we leave out (see the crate docs): no relocation of metadata, so a pair keeps its
//! two blocks for life and the commit path has no retry loop.
//!
//! Namespace operations are in `ops.rs`, file operations in `file.rs`.

use alloc::vec;
use alloc::vec::Vec;

use crate::ctz;
use crate::file::{Content, OpenFile};
use crate::mdir::*;
use crate::tag::{self, *};
use crate::{BlockDevice, Config, Error};

/// Limits this crate writes into new superblocks: the reference's defaults. A name is at
/// most 255 bytes; a file at most 2^31 - 1 bytes (sizes are signed 32-bit in the reference's
/// API); an attribute (and so an inline file) at most 1022 bytes, the most a tag can carry.
const NAME_MAX: u32 = 255;
const FILE_MAX: u32 = 0x7fff_ffff;
const ATTR_MAX: u32 = MAX_TAG_DATA as u32;
const MAGIC: &[u8] = b"littlefs";

/// A tag and its data, as a commit carries it.
pub(crate) type Attr = (u32, Vec<u8>);

/// A metadata pair as fetched: which block is current, where its log ends, what it says.
#[derive(Clone, Debug)]
pub(crate) struct MDir {
    /// `pair[0]` holds the current log; a compaction writes `pair[1]`.
    pub pair: Pair,
    pub rev: u32,
    pub off: u32,
    pub etag: u32,
    pub erased: bool,
    pub c: Contents,
}

/// A file or directory entry's struct, decoded and checked against the volume.
pub(crate) enum Struct {
    Dir(Pair),
    Inline(Vec<u8>),
    Ctz { head: u32, size: u32 },
}

/// A budget of steps for a walk along tails. A volume holds at most `block_count / 2`
/// pairs, so a longer walk has looped: a hostile image may link pairs in a cycle.
pub(crate) struct Walk(u32);

impl Walk {
    /// Call before each pair visited.
    pub fn step(&mut self) -> Result<(), Error> {
        self.0 = self.0.checked_sub(1).ok_or(Error::Corrupt)?;
        Ok(())
    }
}

/// The block allocator (the reference's lookahead allocator, with the window covering the
/// whole volume). Free blocks are found by walking everything in use; a block freed since
/// the last walk is only reused after the next one.
///
/// `ckpoint` counts blocks the allocator may still look at before reporting the volume full.
/// It is reset only when every allocated block is reachable from the disk or an open file
/// (between operations); inside an operation it stops the search from wrapping around onto
/// blocks this same operation already took but has not linked in yet.
#[derive(Default)]
struct Alloc {
    /// The next block to consider.
    next: u32,
    /// Blocks that may still be considered before the volume must be walked again.
    left: u32,
    ckpoint: u32,
    used: Vec<u64>,
}

/// A mounted littlefs volume on a block device.
pub struct Filesystem<D: BlockDevice> {
    dev: D,
    pub(crate) block_size: u32,
    pub(crate) block_count: u32,
    pub(crate) prog_size: u32,
    pub(crate) name_max: u32,
    pub(crate) file_max: u32,
    pub(crate) attr_max: u32,
    pub(crate) inline_max: u32,
    /// The root directory's first pair (the last pair holding a superblock entry).
    pub(crate) root: Pair,
    /// Global state: as on disk, as it should be, and deltas taken from dropped pairs that
    /// the next commit must carry.
    pub(crate) gdisk: GState,
    pub(crate) gstate: GState,
    pub(crate) gdelta: GState,
    alloc: Alloc,
    pub(crate) files: Vec<Option<OpenFile>>,
    /// Generation of the most recently opened file handle.
    pub(crate) file_generation: u32,
    /// Successful metadata commits, to tell whether a failed operation changed the disk.
    commits: u64,
    /// Set when memory may no longer match the disk; everything then fails until a remount.
    poisoned: bool,
    /// The orphan repair has run since mount (see `force_consistency`).
    repaired: bool,
}

impl<D: BlockDevice> Filesystem<D> {
    fn new(dev: D, cfg: Config) -> Result<Self, Error> {
        let Config { block_size, block_count, prog_size } = cfg;
        // Blocks must hold every CTZ pointer a 31-bit file can need (the reference's bound)
        // and a commit's CRC and forward CRC; a pair needs two blocks.
        if block_size < 128 || prog_size == 0 || block_size % prog_size != 0 || block_count < 2 {
            return Err(Error::Invalid);
        }
        let words = (block_count as usize).div_ceil(64);
        Ok(Filesystem {
            dev,
            block_size,
            block_count,
            prog_size,
            name_max: NAME_MAX,
            file_max: FILE_MAX,
            attr_max: ATTR_MAX,
            // Small files live in their directory's metadata (the reference's default bound).
            inline_max: ATTR_MAX.min(block_size / 8),
            root: PAIR_NULL,
            gdisk: GState::default(),
            gstate: GState::default(),
            gdelta: GState::default(),
            alloc: Alloc { next: 0, left: 0, ckpoint: block_count, used: vec![0; words] },
            files: Vec::new(),
            file_generation: 0,
            commits: 0,
            poisoned: false,
            repaired: false,
        })
    }

    // ---- device access: every read is bounds-checked, every failed write poisons ----

    pub(crate) fn bd_read(&mut self, block: u32, off: u32, buf: &mut [u8]) -> Result<(), Error> {
        if block >= self.block_count || off as u64 + buf.len() as u64 > self.block_size as u64 {
            return Err(Error::Corrupt);
        }
        self.dev.read(block, off, buf)
    }

    pub(crate) fn bd_prog(&mut self, block: u32, off: u32, data: &[u8]) -> Result<(), Error> {
        if block >= self.block_count
            || off as u64 + data.len() as u64 > self.block_size as u64
            || !off.is_multiple_of(self.prog_size)
            || !(data.len() as u32).is_multiple_of(self.prog_size)
        {
            return Err(Error::Invalid);
        }
        let r = self.dev.prog(block, off, data);
        self.poisoned |= r.is_err();
        r
    }

    pub(crate) fn bd_erase(&mut self, block: u32) -> Result<(), Error> {
        if block >= self.block_count {
            return Err(Error::Invalid);
        }
        let r = self.dev.erase(block);
        self.poisoned |= r.is_err();
        r
    }

    pub(crate) fn bd_sync(&mut self) -> Result<(), Error> {
        let r = self.dev.sync();
        self.poisoned |= r.is_err();
        r
    }

    pub(crate) fn read_u32(&mut self, block: u32, off: u32) -> Result<u32, Error> {
        let mut b = [0u8; 4];
        self.bd_read(block, off, &mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    /// Reads a block pointer from the medium and checks it points into the volume.
    pub(crate) fn read_pointer(&mut self, block: u32, off: u32) -> Result<u32, Error> {
        let p = self.read_u32(block, off)?;
        if p >= self.block_count { Err(Error::Corrupt) } else { Ok(p) }
    }

    // ---- operations: the poison and rollback rules in one place ----

    pub(crate) fn check_poison(&self) -> Result<(), Error> {
        if self.poisoned { Err(Error::Poisoned) } else { Ok(()) }
    }

    /// Runs `op` so that it either succeeds, or fails having changed nothing on disk (and
    /// then the in-memory global state is rolled back too), or poisons the filesystem.
    fn guarded<T>(&mut self, op: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        let (gstate, gdelta, commits) = (self.gstate, self.gdelta, self.commits);
        let r = op(self);
        if r.is_err() {
            if self.commits != commits {
                self.poisoned = true;
            } else {
                self.gstate = gstate;
                self.gdelta = gdelta;
            }
        }
        r
    }

    /// Every operation that may write goes through here: first finish whatever an
    /// interrupted operation left (at most once per mount), then run `op`.
    pub(crate) fn mutate<T>(&mut self, op: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        self.check_poison()?;
        // Between operations every allocated block is on disk or held by an open file.
        self.alloc_ckpoint();
        self.guarded(|fs| fs.force_consistency())?;
        self.guarded(op)
    }

    // ---- metadata pairs ----

    /// Reads a pair and returns its current block's contents.
    pub(crate) fn fetch(&mut self, pair: Pair) -> Result<MDir, Error> {
        if pair[0] >= self.block_count || pair[1] >= self.block_count {
            return Err(Error::Corrupt);
        }
        let revs = [self.read_u32(pair[0], 0)?, self.read_u32(pair[1], 0)?];
        // Try the newer revision first (sequence comparison: the count may wrap). If it holds
        // no valid commit, a compaction into it was torn and the other block is current.
        let first = if (revs[1].wrapping_sub(revs[0]) as i32) > 0 { 1 } else { 0 };
        let mut data = vec![0u8; self.block_size as usize];
        for i in [first, 1 - first] {
            self.bd_read(pair[i], 0, &mut data)?;
            if let Some(p) = parse_block(&data, self.prog_size)? {
                return Ok(MDir {
                    pair: [pair[i], pair[1 - i]],
                    rev: revs[i],
                    off: p.off,
                    etag: p.etag,
                    erased: p.erased,
                    c: p.contents,
                });
            }
        }
        Err(Error::Corrupt)
    }

    /// Decodes an entry's struct, checking every value against the volume.
    pub(crate) fn decode(&self, e: &Entry) -> Result<Struct, Error> {
        let pointer = |p: u32| if p < self.block_count { Ok(p) } else { Err(Error::Corrupt) };
        match (e.name_type, &e.strct) {
            (TYPE_DIR, Some((TYPE_DIRSTRUCT, d))) if d.len() == 8 => {
                Ok(Struct::Dir([pointer(le32(&d[0..]))?, pointer(le32(&d[4..]))?]))
            }
            (TYPE_REG, Some((TYPE_INLINESTRUCT, d))) => Ok(Struct::Inline(d.clone())),
            (TYPE_REG, Some((TYPE_CTZSTRUCT, d))) if d.len() == 8 => {
                let (head, size) = (le32(&d[0..]), le32(&d[4..]));
                // A file cannot be larger than the volume; this also bounds every walk of
                // its skip-list.
                if size > self.file_max || size as u64 > self.block_count as u64 * self.block_size as u64 {
                    return Err(Error::Corrupt);
                }
                if size > 0 {
                    pointer(head)?;
                }
                Ok(Struct::Ctz { head, size })
            }
            _ => Err(Error::Corrupt),
        }
    }

    pub(crate) fn walk(&self) -> Walk { Walk(self.block_count / 2) }

    /// The pair whose tail is `pair`, if any.
    pub(crate) fn pred(&mut self, pair: &Pair) -> Result<Option<MDir>, Error> {
        let mut tail = [0, 1];
        let mut walk = self.walk();
        while !pair_is_null(&tail) {
            walk.step()?;
            let d = self.fetch(tail)?;
            if pair_overlaps(&d.c.tail, pair) {
                return Ok(Some(d));
            }
            tail = d.c.tail;
        }
        Ok(None)
    }

    /// Where the directory entry naming the directory at `pair` points, if any entry does.
    fn parent_link(&mut self, pair: &Pair) -> Result<Option<Pair>, Error> {
        let mut tail = [0, 1];
        let mut walk = self.walk();
        while !pair_is_null(&tail) {
            walk.step()?;
            let d = self.fetch(tail)?;
            let moved = self.gdisk.move_in(&d.pair);
            for (id, e) in d.c.entries.iter().enumerate() {
                if Some(id as u16) == moved || e.name_type != TYPE_DIR {
                    continue;
                }
                if let Struct::Dir(p) = self.decode(e)? {
                    if pair_overlaps(&p, pair) {
                        return Ok(Some(p));
                    }
                }
            }
            tail = d.c.tail;
        }
        Ok(None)
    }

    // ---- the block allocator ----

    pub(crate) fn alloc_ckpoint(&mut self) { self.alloc.ckpoint = self.block_count; }

    pub(crate) fn alloc_block(&mut self) -> Result<u32, Error> {
        loop {
            while self.alloc.left > 0 {
                let n = self.alloc.next;
                self.alloc.next = (n + 1) % self.block_count;
                self.alloc.left -= 1;
                self.alloc.ckpoint -= 1;
                if self.alloc.used[(n / 64) as usize] & (1 << (n % 64)) == 0 {
                    return Ok(n);
                }
            }
            if self.alloc.ckpoint == 0 {
                return Err(Error::NoSpace);
            }
            self.alloc_scan()?;
        }
    }

    /// Marks every block in use, and lets the search go on for up to `ckpoint` more blocks.
    fn alloc_scan(&mut self) -> Result<(), Error> {
        let mut used = core::mem::take(&mut self.alloc.used);
        used.fill(0);
        let r = self.traverse(&mut |b| {
            used[(b / 64) as usize] |= 1 << (b % 64);
            Ok(())
        });
        self.alloc.used = used;
        r?;
        self.alloc.left = self.block_count.min(self.alloc.ckpoint);
        Ok(())
    }

    /// Calls `f` with every block in use (each one checked to be inside the volume): all
    /// metadata pairs, every file's data blocks, and the blocks open files hold (their old
    /// contents and what they are writing).
    ///
    /// On a valid volume the committed part visits at most `3 * block_count` blocks: every
    /// block holds one pair or one file's data, a directory's pair is seen again through its
    /// entry, and a pending rename shows one file twice. More means forged sizes or looping
    /// pointers (each file alone is bounded by the volume size, but many files claiming the
    /// whole volume would make every allocation scan cost files x blocks), so it is `Corrupt`.
    fn traverse(&mut self, f: &mut dyn FnMut(u32) -> Result<(), Error>) -> Result<(), Error> {
        let limit = 3 * self.block_count as u64;
        let mut visited = 0u64;
        let mut committed = |b: u32| {
            visited += 1;
            if visited > limit { Err(Error::Corrupt) } else { f(b) }
        };
        let mut tail = [0, 1];
        let mut walk = self.walk();
        while !pair_is_null(&tail) {
            walk.step()?;
            let d = self.fetch(tail)?;
            committed(tail[0])?;
            committed(tail[1])?;
            for e in &d.c.entries {
                if !is_file_or_dir(e.name_type) {
                    continue;
                }
                match self.decode(e)? {
                    Struct::Ctz { head, size } => self.ctz_traverse(head, size, None, &mut committed)?,
                    // A directory is also on the tail list, unless it is an orphan.
                    Struct::Dir(p) => {
                        committed(p[0])?;
                        committed(p[1])?;
                    }
                    Struct::Inline(_) => {}
                }
            }
            tail = d.c.tail;
        }

        let mut chains = Vec::new();
        for file in self.files.iter().flatten() {
            if let Content::Ctz { head, size } = file.content {
                chains.push((head, size, None));
            }
            if let Some(w) = &file.writer {
                f(w.block)?;
                // The block being filled is still in memory, pointers included.
                chains.push((w.block, file.pos, Some(w.buf[0..8].to_vec())));
            }
        }
        for (head, size, first) in chains {
            self.ctz_traverse(head, size, first, f)?;
        }
        Ok(())
    }

    /// Calls `f` with each block of a file's skip-list. At most one pointer read per block,
    /// and the file size (checked against the volume) bounds the number of blocks.
    /// `first`: the head block's leading pointers, when that block is still in memory.
    pub(crate) fn ctz_traverse(
        &mut self,
        mut head: u32,
        size: u32,
        mut first: Option<Vec<u8>>,
        f: &mut dyn FnMut(u32) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if size == 0 {
            return Ok(());
        }
        let (mut index, _) = ctz::index(self.block_size, size - 1);
        loop {
            if head >= self.block_count {
                return Err(Error::Corrupt);
            }
            f(head)?;
            if index == 0 {
                return Ok(());
            }
            // An odd block points only to its predecessor; an even one also two back, which
            // lets the walk take both at once.
            let n = 2 - (index & 1);
            let mut heads = [0u32; 2];
            for k in 0..n {
                heads[k as usize] = match &first {
                    Some(b) => le32(&b[4 * k as usize..]),
                    None => self.read_pointer(head, 4 * k)?,
                };
            }
            first = None;
            if n == 2 {
                if heads[0] >= self.block_count {
                    return Err(Error::Corrupt);
                }
                f(heads[0])?;
            }
            head = heads[n as usize - 1];
            index -= n;
        }
    }

    /// Finds the block holding byte `pos` of a file, and the offset in it.
    pub(crate) fn ctz_find(&mut self, mut head: u32, size: u32, pos: u32) -> Result<(u32, u32), Error> {
        if pos >= size {
            return Err(Error::Invalid);
        }
        let (mut current, _) = ctz::index(self.block_size, size - 1);
        let (target, off) = ctz::index(self.block_size, pos);
        while current > target {
            let skip = ctz::jump(current, target);
            head = self.read_pointer(head, 4 * skip)?;
            current -= 1 << skip;
        }
        if head >= self.block_count { Err(Error::Corrupt) } else { Ok((head, off)) }
    }

    // ---- commits ----

    /// Applies `attrs` to the pair `pair` as one atomic commit: appended to its log if there
    /// is room, otherwise by compacting (and splitting, if one block cannot hold it all).
    /// The global state's pending change rides along. Open files are updated to follow.
    pub(crate) fn commit(&mut self, pair: Pair, attrs: &[Attr]) -> Result<(), Error> {
        let dir = self.fetch(pair)?;
        let mut c = dir.c.clone();
        for (t, d) in attrs {
            c.apply(*t, d)?;
        }
        c.check()?;

        // A directory's later pairs disappear when they empty: the pair before takes over
        // the tail, and the global-state delta, of the dropped one.
        let deletes = attrs.iter().any(|(t, _)| tag::type3(*t) == TYPE_DELETE);
        if deletes && c.entries.is_empty() {
            if let Some(pred) = self.pred(&dir.pair)? {
                if pred.c.split {
                    // Files open on the emptied pair were all deleted by this commit; detach
                    // them before the pair's blocks become free for reuse.
                    self.follow_files(&dir.pair, attrs)?;
                    self.gdelta = self.gdelta.xor(&dir.c.gdelta);
                    // This commit deletes nothing, so it cannot drop in turn.
                    return self.commit(pred.pair, &[attr_tail(dir.c.split, dir.c.tail)]);
                }
            }
        }

        let delta = self.gstate.xor(&self.gdisk).xor(&self.gdelta).without_size();
        c.gdelta = dir.c.gdelta.xor(&delta);
        // The reference keeps pairs under 0xff entries, compacting (and so splitting) at
        // that point; this does the same, so each implementation's pairs suit the other.
        let appended = dir.erased && c.entries.len() < 0xff && self.append(&dir, attrs, &delta, &c.gdelta)?;
        if !appended {
            self.compact(&dir, c)?;
        }
        self.gdisk = self.gstate;
        self.gdelta = GState::default();
        self.commits += 1;
        self.follow_files(&dir.pair, attrs)
    }

    /// Appends a commit to the current block's log. `Ok(false)`: it does not fit.
    fn append(&mut self, dir: &MDir, attrs: &[Attr], delta: &GState, block_delta: &GState) -> Result<bool, Error> {
        // Tags may fill the block but for the 8 bytes of the closing CRC tag.
        let mut cb = CommitBuf::new(dir.off, dir.etag, self.block_size - 8);
        let push = |cb: &mut CommitBuf, t: u32, d: &[u8]| match cb.push_tag(t, d) {
            Err(Error::NoSpace) => Ok(false),
            r => r.map(|()| true),
        };
        for (t, d) in attrs {
            if !push(&mut cb, *t, d)? {
                return Ok(false);
            }
        }
        if !delta.is_zero() && !push(&mut cb, tag::mk(TYPE_MOVESTATE, ID_NONE, 12), &block_delta.to_bytes())? {
            return Ok(false);
        }
        let block = dir.pair[0];
        cb.finish(self.block_size, self.prog_size, &mut |off, buf| self.bd_read(block, off, buf))?;
        self.bd_prog(block, cb.start, &cb.buf)?;
        self.bd_sync()?;
        Ok(true)
    }

    /// Rewrites a pair from scratch into its other block. If the entries do not fit in half
    /// a block, the upper half moves to a new pair linked in as a hard tail, repeatedly (the
    /// reference's `lfs_dir_splittingcompact`).
    fn compact(&mut self, dir: &MDir, mut c: Contents) -> Result<(), Error> {
        let bs = self.block_size;
        // The reference's split rule: keep at most half a block of entries per pair (so the
        // log has room to grow before the next compaction), and never more than leaves 40
        // bytes for what a compaction adds: the tail (12), the global state (16), a pending
        // move's delete (4) and the CRC (8).
        let limit = (bs - 40).min(align_up(bs / 2, self.prog_size));
        loop {
            let end = c.entries.len();
            let mut split = 0;
            while end - split > 1 {
                let size = Contents::entries_size(&c.entries[split..])?;
                if end - split < 0xff && size <= limit {
                    break;
                }
                split += (end - split) / 2;
            }
            if split == 0 {
                break;
            }
            let mut t = Contents::new(c.tail, c.split);
            t.entries = c.entries.split_off(split);
            match self.new_pair(&t) {
                Ok(p) => {
                    c.tail = p;
                    c.split = true;
                }
                // No room for a new pair: try to fit everything in this one.
                Err(Error::NoSpace) => {
                    c.entries.append(&mut t.entries);
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        self.write_pair(dir.pair, dir.rev, &c)
    }

    /// Writes `c` as a single commit into `pair[1]` with revision `rev + 1`, making it the
    /// current block of the pair.
    fn write_pair(&mut self, pair: Pair, rev: u32, c: &Contents) -> Result<(), Error> {
        let block = pair[1];
        let mut cb = CommitBuf::new(0, 0xffff_ffff, self.block_size - 8);
        cb.push_raw(&rev.wrapping_add(1).to_le_bytes());
        for (id, e) in c.entries.iter().enumerate() {
            e.tags(id as u16, &mut |t, d| cb.push_tag(t, d))?;
        }
        if !pair_is_null(&c.tail) {
            let (t, d) = attr_tail(c.split, c.tail);
            cb.push_tag(t, &d)?;
        }
        if !c.gdelta.is_zero() {
            cb.push_tag(tag::mk(TYPE_MOVESTATE, ID_NONE, 12), &c.gdelta.to_bytes())?;
        }
        // Everything fits: only now touch the disk.
        self.bd_erase(block)?;
        cb.finish(self.block_size, self.prog_size, &mut |off, buf| self.bd_read(block, off, buf))?;
        self.bd_prog(block, 0, &cb.buf)?;
        self.bd_sync()
    }

    /// Allocates a pair and writes `c` into it. Not linked anywhere yet.
    pub(crate) fn new_pair(&mut self, c: &Contents) -> Result<Pair, Error> {
        let b1 = self.alloc_block()?;
        let b0 = self.alloc_block()?;
        // The block not written now may hold a stale log; ours must win the revision
        // comparison against it.
        let rev = self.read_u32(b0, 0)?;
        self.write_pair([b0, b1], rev, c)?;
        Ok([b1, b0])
    }

    /// After a commit to `pair`, moves open files' ids past the creates and deletes it made,
    /// detaches files it deleted, and follows files whose entry a split moved to a new pair.
    fn follow_files(&mut self, pair: &Pair, attrs: &[Attr]) -> Result<(), Error> {
        let mut moved = Vec::new();
        for (i, slot) in self.files.iter_mut().enumerate() {
            let Some(f) = slot else { continue };
            let Some(loc) = f.loc else { continue };
            if !pair_overlaps(&loc.pair, pair) {
                continue;
            }
            let mut id = Some(loc.id);
            for (t, _) in attrs {
                let (tid, typ) = (tag::id(*t), tag::type3(*t));
                id = match id {
                    Some(id) if typ == TYPE_DELETE && tid == id => None,
                    Some(id) if typ == TYPE_DELETE && tid < id => Some(id - 1),
                    Some(id) if typ == TYPE_CREATE && tid <= id => Some(id + 1),
                    id => id,
                };
            }
            f.loc = id.map(|id| crate::file::Loc { pair: loc.pair, id });
            if id.is_some() {
                moved.push(i);
            }
        }
        for i in moved {
            let Some(Some(f)) = self.files.get(i) else { continue };
            let Some(loc) = f.loc else { continue };
            let (mut p, mut id) = (loc.pair, loc.id);
            let mut walk = self.walk();
            loop {
                let d = self.fetch(p)?;
                if (id as usize) < d.c.entries.len() || !d.c.split {
                    p = d.pair;
                    break;
                }
                id -= d.c.entries.len() as u16;
                walk.step()?;
                p = d.c.tail;
            }
            if let Some(Some(f)) = self.files.get_mut(i) {
                f.loc = Some(crate::file::Loc { pair: p, id });
            }
        }
        Ok(())
    }

    // ---- global state ----

    pub(crate) fn prep_orphans(&mut self, delta: i32) {
        let n = (self.gstate.orphans() as i32 + delta).clamp(0, 0x3ff) as u32;
        let tag = (self.gstate.tag & !0x3ff) | n;
        // On disk only "there are orphans" survives, in bit 31.
        self.gstate.tag = (tag & 0x7fff_ffff) | if n != 0 { 0x8000_0000 } else { 0 };
    }

    pub(crate) fn prep_move(&mut self, m: Option<(u16, Pair)>) {
        let keep = self.gstate.tag & !tag::mk(0x7ff, 0x3ff, 0);
        match m {
            Some((id, pair)) => {
                self.gstate.tag = keep | tag::mk(TYPE_DELETE, id, 0);
                self.gstate.pair = pair;
            }
            None => {
                self.gstate.tag = keep;
                self.gstate.pair = [0, 0];
            }
        }
    }

    // ---- mount, format, repair ----

    /// Formats the device as an empty littlefs 2.1 volume.
    pub fn format(dev: D, cfg: Config) -> Result<(), Error> {
        let mut fs = Self::new(dev, cfg)?;
        let mut c = Contents::new(PAIR_NULL, false);
        c.entries.push(fs.superblock_entry());
        // Write the pair twice, once into each block, so that no older filesystem left in
        // either block can win the revision comparison.
        let rev = fs.read_u32(1, 0)?;
        fs.write_pair([1, 0], rev, &c)?;
        let d = fs.fetch([0, 1])?;
        fs.write_pair(d.pair, d.rev, &c)?;
        Self::mount(fs.dev, cfg).map(|_| ())
    }

    /// Mounts a volume: walks every metadata pair once to find the root and add up the
    /// global state. Repairs are left to the first operation that writes.
    pub fn mount(dev: D, cfg: Config) -> Result<Self, Error> {
        let mut fs = Self::new(dev, cfg)?;
        let mut tail = [0, 1];
        let mut walk = fs.walk();
        let mut gstate = GState::default();
        while !pair_is_null(&tail) {
            walk.step()?;
            let d = fs.fetch(tail)?;
            if let Some(e) = d.c.entries.first() {
                if e.name_type == TYPE_SUPERBLOCK && e.name == MAGIC {
                    fs.read_superblock(e, cfg)?;
                    fs.root = d.pair;
                }
            }
            gstate = gstate.xor(&d.c.gdelta);
            tail = d.c.tail;
        }
        if pair_is_null(&fs.root) {
            return Err(Error::Corrupt);
        }

        // The length bits are never on disk; in memory they count orphans. Bit 31 says there
        // may be some, so start from one.
        let mut g = gstate.without_size();
        if !tag::is_valid(g.tag) {
            g.tag |= 1;
        }
        fs.gstate = g;
        fs.gdisk = g;
        Ok(fs)
    }

    /// Hands the device back. Open files are dropped unsynced, which is as safe as a power
    /// loss.
    pub fn unmount(self) -> D { self.dev }

    fn read_superblock(&mut self, e: &Entry, cfg: Config) -> Result<(), Error> {
        let Some((TYPE_INLINESTRUCT, d)) = &e.strct else { return Err(Error::Corrupt) };
        if d.len() < 24 {
            return Err(Error::Corrupt);
        }
        let field = |i: usize| le32(&d[4 * i..]);
        if field(0) != crate::DISK_VERSION || field(1) != cfg.block_size || field(2) != cfg.block_count {
            return Err(Error::Invalid);
        }
        // Zero means "the default"; anything else must be within what the format allows.
        let (name_max, file_max, attr_max) = (field(3), field(4), field(5));
        // A superblock may allow longer names than we write, up to what a tag can carry.
        if name_max > MAX_TAG_DATA as u32 || file_max > FILE_MAX || attr_max > ATTR_MAX {
            return Err(Error::Invalid);
        }
        if name_max != 0 {
            self.name_max = name_max;
        }
        if file_max != 0 {
            self.file_max = file_max;
        }
        if attr_max != 0 {
            self.attr_max = attr_max;
            self.inline_max = self.inline_max.min(attr_max);
        }
        Ok(())
    }

    fn superblock_entry(&self) -> Entry {
        let fields = [crate::DISK_VERSION, self.block_size, self.block_count, self.name_max, self.file_max, self.attr_max];
        let strct = Some((TYPE_INLINESTRUCT, fields.iter().flat_map(|v| v.to_le_bytes()).collect()));
        Entry { name_type: TYPE_SUPERBLOCK, name: MAGIC.to_vec(), strct, attrs: Vec::new() }
    }

    /// Completes what an interrupted operation left behind (the reference's
    /// `lfs_fs_forceconsistency`): a half-done rename, orphans.
    fn force_consistency(&mut self) -> Result<(), Error> {
        if self.gdisk.has_move() {
            // A rename created the new entry but did not delete the old one: delete it now.
            if tag::type3(self.gdisk.tag) != TYPE_DELETE {
                return Err(Error::Corrupt);
            }
            let (pair, id) = (self.gdisk.pair, tag::id(self.gdisk.tag));
            // Only a file or directory can have been renamed; never delete anything else
            // (the superblock entry, above all) on the medium's say-so.
            let dir = self.fetch(pair)?;
            if !dir.c.entries.get(id as usize).is_some_and(|e| is_file_or_dir(e.name_type)) {
                return Err(Error::Corrupt);
            }
            self.prep_move(None);
            self.commit(pair, &[attr_delete(id)])?;
        }
        // The orphan repair runs once per mount even when the orphan flag is clear: the C
        // reference can leave orphaned or half-orphaned directories on the list with the flag
        // cleared (a relocation during a removal resets its count). Repairing them before the
        // first write, rather than refusing the volume, keeps its images usable.
        if self.gstate.orphans() != 0 || !self.repaired {
            self.deorphan()?;
            self.repaired = true;
        }
        Ok(())
    }

    /// Repairs the list of pairs (the reference's `lfs_fs_deorphan`). Pass 0 fixes
    /// half-orphans: a directory whose entry points to a pair other than the one on the list
    /// (the reference's relocation moved it). Pass 1 unlinks full orphans: directories on
    /// the list that no entry names (their removal was interrupted).
    ///
    /// Cost, on any image: each pass takes at most `block_count` steps, and each step that
    /// starts a directory searches the whole list for its parent: O(pairs^2) fetches, the
    /// same bound as the reference. A hostile list cannot make it loop or grow further.
    pub(crate) fn deorphan(&mut self) -> Result<(), Error> {
        for pass in 0..2 {
            // Every step either advances or fixes one pair, and each pair is fixed at most
            // once per pass: twice the budget of a plain walk.
            let mut walk = Walk(self.block_count);
            let mut prev: Option<MDir> = None; // before {0, 1}: as if a hard tail led there
            let mut tail = [0, 1];
            while !pair_is_null(&tail) {
                walk.step()?;
                let dir = self.fetch(tail)?;
                if let Some(p) = prev.as_ref().filter(|p| !p.c.split) {
                    // `dir` starts a directory: some entry should name it.
                    let link = self.parent_link(&tail)?;
                    let fix = match link {
                        Some(to) if pass == 0 && !pair_same(&to, &tail) => Some(attr_tail(false, to)),
                        None if pass == 1 => {
                            self.gdelta = self.gdelta.xor(&dir.c.gdelta);
                            Some(attr_tail(dir.c.split, dir.c.tail))
                        }
                        _ => None,
                    };
                    if let Some(fix) = fix {
                        let ppair = p.pair;
                        self.commit(ppair, &[fix])?;
                        let p = self.fetch(ppair)?;
                        tail = p.c.tail;
                        prev = Some(p);
                        continue;
                    }
                }
                tail = dir.c.tail;
                prev = Some(dir);
            }
        }
        let n = self.gstate.orphans() as i32;
        self.prep_orphans(-n);
        Ok(())
    }
}

/// Lets a caller lend a device: `Filesystem<&mut Dev>` gives it back on unmount or failure.
impl<T: BlockDevice + ?Sized> BlockDevice for &mut T {
    fn read(&mut self, block: u32, off: u32, buf: &mut [u8]) -> Result<(), Error> { (**self).read(block, off, buf) }

    fn prog(&mut self, block: u32, off: u32, data: &[u8]) -> Result<(), Error> { (**self).prog(block, off, data) }

    fn erase(&mut self, block: u32) -> Result<(), Error> { (**self).erase(block) }

    fn sync(&mut self) -> Result<(), Error> { (**self).sync() }
}

// ---- building commits ----

pub(crate) fn attr_create(id: u16) -> Attr { (tag::mk(TYPE_CREATE, id, 0), Vec::new()) }

pub(crate) fn attr_delete(id: u16) -> Attr { (tag::mk(TYPE_DELETE, id, 0), Vec::new()) }

pub(crate) fn attr_name(typ: u16, id: u16, name: &[u8]) -> Result<Attr, Error> {
    Ok((tag::mk(typ, id, len16(name)?), name.to_vec()))
}

pub(crate) fn attr_struct(typ: u16, id: u16, data: &[u8]) -> Result<Attr, Error> {
    Ok((tag::mk(typ, id, len16(data)?), data.to_vec()))
}

pub(crate) fn attr_tail(split: bool, pair: Pair) -> Attr {
    (tag::mk(TYPE_SOFTTAIL + split as u16, ID_NONE, 8), pair_bytes(pair))
}

pub(crate) fn pair_bytes(p: Pair) -> Vec<u8> { [p[0].to_le_bytes(), p[1].to_le_bytes()].concat() }
