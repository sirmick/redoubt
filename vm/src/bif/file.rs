//! Files: the NIFs of OTP's `prim_file` and `prim_buffer`, over [`Files`](crate::platform::Files).
//!
//! OTP's own `file`, `file_server` and `file_io_server` run unchanged on top of these, so file
//! semantics (modes, read-ahead, line reading, `consult`, ...) are OTP's. What is here is the
//! thin layer that turns names into platform paths and calls the platform.
//!
//! Names are resolved inside the VM before the platform sees them: relative names against the
//! VM's own working directory (`file:set_cwd/1` changes it for this VM only), then `.` and `..`
//! are resolved lexically, so no name can climb above `/`. The platform decides what `/` is.
//!
//! Every open file belongs to the process that opened it and is closed when that process exits.

use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use super::Ctx;
use crate::platform::{FileError, FileInfo, FileKind, Files, OpenMode, SeekFrom};
use crate::process::Exception;
use crate::term::{Bits, Resource, Term};

type R = Result<Term, Exception>;

/// Most files one VM may have open at once (`emfile` beyond).
pub const MAX_OPEN_FILES: usize = 1024;

/// Longest path, in bytes, after resolution (`enametoolong` beyond).
const MAX_PATH: usize = 4096;

/// An open file, as the `FileRef` resource `prim_file` holds.
struct FileRef {
    handle: u64,
    open: Cell<bool>,
}

// ---- results ----

fn error(c: &mut Ctx, e: FileError) -> Term {
    let reason = c.atom(e.name());
    Term::tuple(alloc::vec![c.atom("error"), reason])
}

fn ok_with(c: &mut Ctx, v: Term) -> Term {
    Term::tuple(alloc::vec![c.ok(), v])
}

/// `ok` or `{error, Reason}`.
fn done(c: &mut Ctx, r: Result<(), FileError>) -> R {
    Ok(match r {
        Ok(()) => c.ok(),
        Err(e) => error(c, e),
    })
}

fn files<'c>(c: &'c mut Ctx) -> Result<&'c mut dyn Files, FileError> {
    c.sys.platform.files().ok_or(FileError::Enotsup)
}

// ---- names ----

/// `internal_name2native(Name)`: a name (a string, possibly deep, or a binary) as UTF-8 bytes.
pub fn name2native(c: &mut Ctx, a: &[Term]) -> R {
    let bytes = native_name(&a[0]).ok_or_else(|| c.badarg())?;
    Ok(Term::binary(&bytes))
}

/// A file name argument (string, deep list, binary or atom) as UTF-8 bytes.
pub(crate) fn name_bytes(t: &Term) -> Option<Vec<u8>> {
    native_name(t)
}

fn native_name(t: &Term) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut work = alloc::vec![t];
    while let Some(t) = work.pop() {
        match t {
            Term::Nil => {}
            Term::Cons(cell) => {
                work.push(&cell.tail);
                work.push(&cell.head);
            }
            Term::Int(ch) => {
                let ch = char::from_u32(u32::try_from(*ch).ok()?)?;
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            Term::Bits(b) if b.is_binary() => out.extend_from_slice(&b.to_bytes()),
            Term::Atom(a) => out.extend_from_slice(a.as_str().as_bytes()),
            _ => return None,
        }
        if out.len() > MAX_PATH {
            return None;
        }
    }
    // A NUL would end the name early on a C platform: BEAM refuses it, and so do we.
    (!out.contains(&0)).then_some(out)
}

fn chars(s: &str) -> Term {
    Term::list(s.chars().map(|ch| Term::Int(ch as i64)).collect::<Vec<_>>())
}

/// `internal_native2name(Bin)`: the name as a string, or `{error, ignore}` if it is not UTF-8.
pub fn native2name(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Bits(b) = &a[0] else { return Err(c.badarg()) };
    match core::str::from_utf8(&b.to_bytes()) {
        Ok(s) => Ok(chars(s)),
        Err(_) => Ok(Term::tuple(alloc::vec![c.atom("error"), c.atom("ignore")])),
    }
}

/// `internal_normalize_utf8(Bin)`: the string. Names are not normalized (as on Linux).
pub fn normalize_utf8(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Bits(b) = &a[0] else { return Err(c.badarg()) };
    let bytes = b.to_bytes();
    let s = core::str::from_utf8(&bytes).map_err(|_| c.badarg())?;
    Ok(chars(s))
}

pub fn is_translatable(c: &mut Ctx, a: &[Term]) -> R {
    let ok = match &a[0] {
        Term::Bits(b) => core::str::from_utf8(&b.to_bytes()).is_ok(),
        _ => true,
    };
    Ok(c.bool(ok))
}

/// `file:native_name_encoding()`: always `utf8`.
pub fn native_name_encoding(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.atom("utf8"))
}

/// Resolve `name` (bytes from `internal_name2native`) against `cwd`: an absolute path with no
/// `.`, `..` or empty components. `..` at the root stays at the root.
pub fn resolve(cwd: &str, name: &[u8]) -> Result<String, FileError> {
    let name = core::str::from_utf8(name).map_err(|_| FileError::Einval)?;
    if name.is_empty() {
        return Err(FileError::Enoent);
    }
    let mut parts: Vec<&str> = Vec::new();
    let start = if name.starts_with('/') { "" } else { cwd };
    for part in start.split('/').chain(name.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let mut path = String::new();
    for p in &parts {
        path.push('/');
        path.push_str(p);
    }
    if path.is_empty() {
        path.push('/');
    }
    if path.len() > MAX_PATH {
        return Err(FileError::Enametoolong);
    }
    Ok(path)
}

/// The platform path an encoded name argument stands for.
fn path(c: &Ctx, t: &Term) -> Result<Result<String, FileError>, Exception> {
    let Term::Bits(b) = t else { return Err(c.badarg()) };
    Ok(resolve(&c.sys.cwd, &b.to_bytes()))
}

/// Run `f` on the resolved path, turning a failure into `{error, Reason}`.
fn with_path(c: &mut Ctx, t: &Term, f: impl FnOnce(&mut Ctx, &str) -> R) -> R {
    match path(c, t)? {
        Ok(p) => f(c, &p),
        Err(e) => Ok(error(c, e)),
    }
}

// ---- file information ----

fn info_term(c: &mut Ctx, i: &FileInfo) -> Term {
    let kind = match i.kind {
        FileKind::Regular => "regular",
        FileKind::Directory => "directory",
        FileKind::Symlink => "symlink",
        FileKind::Other => "other",
    };
    let access = match (i.readable, i.writable) {
        (true, true) => "read_write",
        (true, false) => "read",
        (false, true) => "write",
        (false, false) => "none",
    };
    let int = |n: i64| Term::Int(n);
    Term::tuple(alloc::vec![
        c.atom("file_info"),
        Term::big(i.size.into()),
        c.atom(kind),
        c.atom(access),
        int(i.atime),
        int(i.mtime),
        int(i.ctime),
        int(i.mode as i64),
        Term::big(i.links.into()),
        int(0),
        int(0),
        Term::big(i.inode.into()),
        int(i.uid as i64),
        int(i.gid as i64),
    ])
}

/// `read_info_nif(Path, FollowLinks)`: a `#file_info{}` with POSIX times, or `{error, R}`.
pub fn read_info(c: &mut Ctx, a: &[Term]) -> R {
    let follow = !matches!(a[1], Term::Int(0));
    with_path(c, &a[0], |c, p| {
        let r = files(c).and_then(|f| f.info(p, follow));
        Ok(match r {
            Ok(i) => info_term(c, &i),
            Err(e) => error(c, e),
        })
    })
}

pub fn read_handle_info(c: &mut Ctx, a: &[Term]) -> R {
    let h = handle(c, &a[0])?;
    let r = h.and_then(|h| files(c)?.handle_info(h));
    Ok(match r {
        Ok(i) => info_term(c, &i),
        Err(e) => error(c, e),
    })
}

// ---- directories and names ----

pub fn list_dir(c: &mut Ctx, a: &[Term]) -> R {
    with_path(c, &a[0], |c, p| {
        Ok(match files(c).and_then(|f| f.list_dir(p)) {
            Ok(names) => {
                let names: Vec<Term> = names.iter().map(|n| Term::binary(n)).collect();
                ok_with(c, Term::list(names))
            }
            Err(e) => error(c, e),
        })
    })
}

pub fn make_dir(c: &mut Ctx, a: &[Term]) -> R {
    with_path(c, &a[0], |c, p| {
        let r = files(c).and_then(|f| f.make_dir(p));
        done(c, r)
    })
}

pub fn del_file(c: &mut Ctx, a: &[Term]) -> R {
    with_path(c, &a[0], |c, p| {
        let r = files(c).and_then(|f| f.delete(p));
        done(c, r)
    })
}

/// `del_dir_nif(Path)`. A directory that is not empty is `eexist`, as OTP reports it.
pub fn del_dir(c: &mut Ctx, a: &[Term]) -> R {
    with_path(c, &a[0], |c, p| {
        let r = files(c).and_then(|f| f.del_dir(p)).map_err(|e| match e {
            FileError::Enotempty => FileError::Eexist,
            e => e,
        });
        done(c, r)
    })
}

pub fn rename(c: &mut Ctx, a: &[Term]) -> R {
    let (from, to) = (path(c, &a[0])?, path(c, &a[1])?);
    let r = from.and_then(|from| to.and_then(|to| files(c)?.rename(&from, &to)));
    done(c, r)
}

pub fn read_link(c: &mut Ctx, a: &[Term]) -> R {
    with_path(c, &a[0], |c, p| {
        Ok(match files(c).and_then(|f| f.read_link(p)) {
            Ok(target) => ok_with(c, Term::binary(&target)),
            Err(e) => error(c, e),
        })
    })
}

/// `get_cwd_nif()`: `{error, enoent}` if the directory has since been removed, as `getcwd` says.
pub fn get_cwd(c: &mut Ctx, _a: &[Term]) -> R {
    let cwd = c.sys.cwd.clone();
    if let Some(Err(e)) = c.sys.platform.files().map(|f| f.info(&cwd, true)) {
        return Ok(error(c, e));
    }
    Ok(ok_with(c, Term::binary(cwd.as_bytes())))
}

/// `set_cwd_nif(Path)`: this VM's working directory, which must be a directory.
pub fn set_cwd(c: &mut Ctx, a: &[Term]) -> R {
    with_path(c, &a[0], |c, p| {
        let r = files(c).and_then(|f| f.info(p, true)).and_then(|i| match i.kind {
            FileKind::Directory => Ok(()),
            _ => Err(FileError::Enotdir),
        });
        if r.is_ok() {
            c.sys.cwd = p.into();
        }
        done(c, r)
    })
}

/// `set_time_nif(Path, ATime, MTime, CTime)`: times in POSIX seconds (the change time cannot be
/// set and is ignored, as on BEAM).
pub fn set_time(c: &mut Ctx, a: &[Term]) -> R {
    let (Term::Int(at), Term::Int(mt)) = (&a[1], &a[2]) else { return Err(c.badarg()) };
    let (at, mt) = (*at, *mt);
    with_path(c, &a[0], |c, p| {
        let r = files(c).and_then(|f| f.set_times(p, at, mt));
        done(c, r)
    })
}

pub fn set_permissions(c: &mut Ctx, a: &[Term]) -> R {
    let mode = match a[1] {
        Term::Int(m) => u32::try_from(m).map_err(|_| c.badarg())?,
        _ => return Err(c.badarg()),
    };
    with_path(c, &a[0], |c, p| {
        let r = files(c).and_then(|f| f.set_permissions(p, mode & 0o7777));
        done(c, r)
    })
}

/// `make_soft_link_nif(Target, Link)`: the target is stored as written.
pub fn make_symlink(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Bits(target) = &a[0] else { return Err(c.badarg()) };
    let target = target.to_bytes().into_owned();
    with_path(c, &a[1], |c, p| {
        let r = files(c).and_then(|f| f.make_symlink(&target, p));
        done(c, r)
    })
}

pub fn make_link(c: &mut Ctx, a: &[Term]) -> R {
    let (from, to) = (path(c, &a[0])?, path(c, &a[1])?);
    let r = from.and_then(|from| to.and_then(|to| files(c)?.make_link(&from, &to)));
    done(c, r)
}

/// NIFs for things the VM does not offer (ownership, raw handles, Windows device paths):
/// `{error, enotsup}`, which `file:write_file_info/2` tolerates.
pub fn not_supported(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(error(c, FileError::Enotsup))
}

// ---- open files ----

/// `open_nif(Path, Modes)`: `{ok, FileRef}` or `{error, Reason}`.
pub fn open(c: &mut Ctx, a: &[Term]) -> R {
    let modes = a[1].to_vec().ok_or_else(|| c.badarg())?;
    let mut m = OpenMode::default();
    for mode in &modes {
        match mode {
            Term::Atom(x) => match x.as_str() {
                "read" => m.read = true,
                "write" => m.write = true,
                "append" => m.append = true,
                "exclusive" => m.exclusive = true,
                // Options `prim_file` or `file` handle themselves, or hints.
                "binary" | "raw" | "read_ahead" | "delayed_write" | "sync" | "compressed" | "ram" | "directory" => {}
                _ => return Err(c.badarg()),
            },
            Term::Tuple(_) => {}
            _ => return Err(c.badarg()),
        }
    }
    if m.append || m.exclusive {
        m.write = true;
    }
    if !m.write {
        m.read = true;
    }
    m.create = m.write;
    // `write` alone empties the file; with `read` or `append` it keeps the contents.
    m.truncate = m.write && !m.read && !m.append;
    with_path(c, &a[0], |c, p| {
        if c.sys.files.len() >= MAX_OPEN_FILES {
            return Ok(error(c, FileError::Emfile));
        }
        match files(c).and_then(|f| f.open(p, m)) {
            Ok(h) => {
                let owner = c.p.pid;
                c.sys.files.insert(h, owner);
                let id = c.sys.make_ref().0;
                let r = Term::Resource(Rc::new(Resource {
                    id,
                    value: alloc::boxed::Box::new(FileRef { handle: h, open: Cell::new(true) }),
                }));
                Ok(ok_with(c, r))
            }
            Err(e) => Ok(error(c, e)),
        }
    })
}

/// The platform handle of an open `FileRef`; `ebadf` once closed. Only the process that opened
/// the file may use it (BEAM checks the owner in `prim_file`; the check here backs that up).
fn handle(c: &Ctx, t: &Term) -> Result<Result<u64, FileError>, Exception> {
    let Term::Resource(r) = t else { return Err(c.badarg()) };
    let f = r.get::<FileRef>().ok_or_else(|| c.badarg())?;
    if !f.open.get() {
        return Ok(Err(FileError::Ebadf));
    }
    if c.sys.files.get(&f.handle) != Some(&c.p.pid) {
        return Err(c.badarg());
    }
    Ok(Ok(f.handle))
}

pub fn close(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Resource(r) = &a[0] else { return Err(c.badarg()) };
    let f = r.get::<FileRef>().ok_or_else(|| c.badarg())?;
    if !f.open.replace(false) {
        return Ok(error(c, FileError::Einval));
    }
    c.sys.files.remove(&f.handle);
    if let Ok(fs) = files(c) {
        fs.close(f.handle);
    }
    Ok(c.ok())
}

/// Largest read one call may ask for: what fits in a binary.
fn read_size(c: &Ctx, t: &Term) -> Result<usize, Exception> {
    let n = t.as_usize().ok_or_else(|| c.badarg())?;
    Ok(n.min(c.sys.limits.max_binary_bits / 8))
}

fn data(c: &mut Ctx, r: Result<Vec<u8>, FileError>) -> Term {
    match r {
        Ok(d) if d.is_empty() => c.atom("eof"),
        Ok(d) => ok_with(c, Term::binary(&d)),
        Err(e) => error(c, e),
    }
}

pub fn read(c: &mut Ctx, a: &[Term]) -> R {
    let (h, len) = (handle(c, &a[0])?, read_size(c, &a[1])?);
    if len == 0 {
        return Ok(ok_with(c, Term::binary(&[])));
    }
    let r = h.and_then(|h| files(c)?.read(h, len));
    Ok(data(c, r))
}

pub fn pread(c: &mut Ctx, a: &[Term]) -> R {
    let h = handle(c, &a[0])?;
    let off = offset(c, &a[1])?;
    let len = read_size(c, &a[2])?;
    if len == 0 {
        return Ok(ok_with(c, Term::binary(&[])));
    }
    let r = h.and_then(|h| files(c)?.pread(h, off, len));
    Ok(data(c, r))
}

fn offset(c: &Ctx, t: &Term) -> Result<u64, Exception> {
    match t {
        Term::Int(n) => u64::try_from(*n).map_err(|_| c.badarg()),
        _ => Err(c.badarg()),
    }
}

/// The bytes of an iovec (a list of binaries).
fn iovec(c: &Ctx, t: &Term) -> Result<Vec<u8>, Exception> {
    let mut out = Vec::new();
    for b in t.to_vec().ok_or_else(|| c.badarg())? {
        match &b {
            Term::Bits(b) if b.is_binary() => out.extend_from_slice(&b.to_bytes()),
            _ => return Err(c.badarg()),
        }
    }
    Ok(out)
}

pub fn write(c: &mut Ctx, a: &[Term]) -> R {
    let h = handle(c, &a[0])?;
    let bytes = iovec(c, &a[1])?;
    let r = h.and_then(|h| files(c)?.write(h, &bytes));
    done(c, r)
}

pub fn pwrite(c: &mut Ctx, a: &[Term]) -> R {
    let h = handle(c, &a[0])?;
    let off = offset(c, &a[1])?;
    let bytes = iovec(c, &a[2])?;
    let r = h.and_then(|h| files(c)?.pwrite(h, off, &bytes));
    done(c, r)
}

/// `seek_nif(FileRef, bof | cur | eof, Offset)`: `{ok, NewPosition}`.
pub fn seek(c: &mut Ctx, a: &[Term]) -> R {
    let h = handle(c, &a[0])?;
    let Term::Int(off) = a[2] else { return Err(c.badarg()) };
    let to = match &a[1] {
        Term::Atom(m) if m.as_str() == "bof" => match u64::try_from(off) {
            Ok(o) => SeekFrom::Start(o),
            Err(_) => return Ok(error(c, FileError::Einval)),
        },
        Term::Atom(m) if m.as_str() == "cur" => SeekFrom::Current(off),
        Term::Atom(m) if m.as_str() == "eof" => SeekFrom::End(off),
        _ => return Err(c.badarg()),
    };
    Ok(match h.and_then(|h| files(c)?.seek(h, to)) {
        Ok(pos) => ok_with(c, Term::big(pos.into())),
        Err(e) => error(c, e),
    })
}

pub fn sync(c: &mut Ctx, a: &[Term]) -> R {
    let h = handle(c, &a[0])?;
    let r = h.and_then(|h| files(c)?.sync(h));
    done(c, r)
}

pub fn truncate(c: &mut Ctx, a: &[Term]) -> R {
    let h = handle(c, &a[0])?;
    let r = h.and_then(|h| files(c)?.truncate(h));
    done(c, r)
}

/// `advise_nif`: a hint, accepted and ignored.
pub fn advise(c: &mut Ctx, a: &[Term]) -> R {
    let h = handle(c, &a[0])?;
    done(c, h.map(|_| ()))
}

/// The whole file at `path` (already resolved), if it is at most `max` bytes.
pub fn read_whole_file(f: &mut dyn Files, path: &str, max: usize) -> Result<Vec<u8>, FileError> {
    let size = f.info(path, true)?.size;
    if size > max as u64 {
        return Err(FileError::Einval);
    }
    let h = f.open(path, OpenMode { read: true, ..OpenMode::default() })?;
    let mut out = Vec::new();
    let r = loop {
        match f.read(h, 1 << 16) {
            Ok(d) if d.is_empty() => break Ok(out),
            Ok(d) if out.len() + d.len() > max => break Err(FileError::Einval),
            Ok(d) => out.extend_from_slice(&d),
            Err(e) => break Err(e),
        }
    };
    f.close(h);
    r
}

/// `read_file_nif(Path)`: the whole file. Files larger than a binary may be are `enomem`
/// in BEAM; here `einval` (the file is not read at all).
pub fn read_file(c: &mut Ctx, a: &[Term]) -> R {
    let max = c.sys.limits.max_binary_bits / 8;
    with_path(c, &a[0], |c, p| {
        let r = files(c).and_then(|f| read_whole_file(f, p, max));
        Ok(match r {
            Ok(d) => ok_with(c, Term::binary(&d)),
            Err(e) => error(c, e),
        })
    })
}

// ---- prim_buffer ----

/// A byte queue: `prim_file`'s read-ahead buffer.
struct Buffer {
    bytes: RefCell<VecDeque<u8>>,
    locked: Cell<bool>,
}

fn buffer<'t>(c: &Ctx, t: &'t Term) -> Result<&'t Buffer, Exception> {
    match t {
        Term::Resource(r) => r.get::<Buffer>().ok_or_else(|| c.badarg()),
        _ => Err(c.badarg()),
    }
}

pub fn buffer_new(c: &mut Ctx, _a: &[Term]) -> R {
    let id = c.sys.make_ref().0;
    let b = Buffer { bytes: RefCell::new(VecDeque::new()), locked: Cell::new(false) };
    Ok(Term::Resource(Rc::new(Resource { id, value: alloc::boxed::Box::new(b) })))
}

pub fn buffer_size(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int(buffer(c, &a[0])?.bytes.borrow().len() as i64))
}

/// `peek_head(Buffer)`: the buffer's contents, left in place.
pub fn buffer_peek_head(c: &mut Ctx, a: &[Term]) -> R {
    let b = buffer(c, &a[0])?;
    let mut bytes = b.bytes.borrow_mut();
    Ok(Term::bits(Bits::from_bytes(bytes.make_contiguous())))
}

/// `copying_read(Buffer, Size)`: the first `Size` bytes, removed.
pub fn buffer_copying_read(c: &mut Ctx, a: &[Term]) -> R {
    let b = buffer(c, &a[0])?;
    let n = a[1].as_usize().ok_or_else(|| c.badarg())?;
    let mut bytes = b.bytes.borrow_mut();
    if n > bytes.len() {
        return Err(c.badarg());
    }
    let out: Vec<u8> = bytes.drain(..n).collect();
    Ok(Term::binary(&out))
}

pub fn buffer_write(c: &mut Ctx, a: &[Term]) -> R {
    let data = iovec(c, &a[1])?;
    let b = buffer(c, &a[0])?;
    let mut bytes = b.bytes.borrow_mut();
    if (bytes.len() + data.len()).saturating_mul(8) > c.sys.limits.max_binary_bits {
        return Err(c.system_limit());
    }
    bytes.extend(data);
    Ok(c.ok())
}

pub fn buffer_skip(c: &mut Ctx, a: &[Term]) -> R {
    let b = buffer(c, &a[0])?;
    let n = a[1].as_usize().ok_or_else(|| c.badarg())?;
    let mut bytes = b.bytes.borrow_mut();
    if n > bytes.len() {
        return Err(c.badarg());
    }
    bytes.drain(..n);
    Ok(c.ok())
}

/// `find_byte_index(Buffer, Byte)`: `{ok, Index}` or `not_found`.
pub fn buffer_find_byte_index(c: &mut Ctx, a: &[Term]) -> R {
    let b = buffer(c, &a[0])?;
    let needle = match a[1] {
        Term::Int(n @ 0..=255) => n as u8,
        _ => return Err(c.badarg()),
    };
    let found = b.bytes.borrow().iter().position(|&x| x == needle);
    Ok(match found {
        Some(i) => ok_with(c, Term::Int(i as i64)),
        None => c.atom("not_found"),
    })
}

pub fn buffer_try_lock(c: &mut Ctx, a: &[Term]) -> R {
    let b = buffer(c, &a[0])?;
    let got = !b.locked.replace(true);
    Ok(c.atom(if got { "acquired" } else { "busy" }))
}

pub fn buffer_unlock(c: &mut Ctx, a: &[Term]) -> R {
    buffer(c, &a[0])?.locked.set(false);
    Ok(c.ok())
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use crate::platform::FileError;

    #[test]
    fn names_resolve_inside_the_root() {
        assert_eq!(resolve("/", b"a/b").unwrap(), "/a/b");
        assert_eq!(resolve("/home", b"x").unwrap(), "/home/x");
        assert_eq!(resolve("/home", b"/x").unwrap(), "/x");
        assert_eq!(resolve("/home", b"../../../etc/passwd").unwrap(), "/etc/passwd");
        assert_eq!(resolve("/a/b", b"./c/./../d//e/").unwrap(), "/a/b/d/e");
        assert_eq!(resolve("/a", b"..").unwrap(), "/");
        assert_eq!(resolve("/", b"").unwrap_err(), FileError::Enoent);
        assert_eq!(resolve("/", &[0xff]).unwrap_err(), FileError::Einval);
    }
}
