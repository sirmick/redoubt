//! The POSIX file system for a VM: host directories mounted into the VM's name space. One is
//! `/` (`--root`); others appear at their mount points (`--mount /otp=DIR:ro`), read-only if
//! asked, like the per-process namespaces of the OS this is for.
//!
//! Every path is opened relative to its mount's directory through `cap-std`, which refuses
//! anything that would leave it: `..` past the top (the VM resolves those already) and, what
//! the VM cannot see, symbolic links pointing outside. An escape attempt fails with `eacces`.

use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Seek, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileExt;

use beamlet_vm::platform::{FileError, FileInfo, FileKind, Files, OpenMode, SeekFrom};
use cap_std::fs::{Dir, MetadataExt, OpenOptions};

struct Mount {
    /// Where it appears: `/`, or `/otp` (no trailing slash).
    at: String,
    /// The host directory, canonical.
    host: std::path::PathBuf,
    dir: Dir,
    read_only: bool,
}

pub struct HostDir {
    /// Longest mount point first, so the first match is the most specific.
    mounts: Vec<Mount>,
    open: BTreeMap<u64, std::fs::File>,
    next: u64,
}

impl HostDir {
    /// A file system whose `/` is the host directory `dir`.
    pub fn new(dir: &str) -> std::io::Result<HostDir> {
        let mut fs = HostDir { mounts: Vec::new(), open: BTreeMap::new(), next: 1 };
        fs.mount("/", dir, false)?;
        Ok(fs)
    }

    /// Show host directory `dir` at VM path `at` (absolute, e.g. `/otp`).
    pub fn mount(&mut self, at: &str, dir: &str, read_only: bool) -> std::io::Result<()> {
        let at = format!("/{}", at.trim_matches('/'));
        let host = std::fs::canonicalize(dir)?;
        let dir = Dir::open_ambient_dir(dir, cap_std::ambient_authority())?;
        self.mounts.retain(|m| m.at != at);
        self.mounts.push(Mount { at, host, dir, read_only });
        self.mounts.sort_by_key(|m| std::cmp::Reverse(m.at.len()));
        Ok(())
    }

    /// The VM path of a host file, if some mount shows it (the most specific one).
    pub fn vm_path(&self, host_file: &std::path::Path) -> Option<String> {
        let host_file = std::fs::canonicalize(host_file).ok()?;
        self.mounts
            .iter()
            .filter_map(|m| host_file.strip_prefix(&m.host).ok().map(|rest| (m, rest)))
            .min_by_key(|(_, rest)| rest.components().count())
            .and_then(|(m, rest)| {
                let rest = rest.to_str()?;
                Some(if m.at == "/" { format!("/{rest}") } else { format!("{}/{rest}", m.at) })
            })
    }

    /// The mount a VM path (`/a/b`, already normalized) is in, and the path within it.
    fn at<'p>(&self, path: &'p str) -> (&Mount, &'p str) {
        let m = self
            .mounts
            .iter()
            .find(|m| m.at == "/" || path == m.at || path.strip_prefix(m.at.as_str()).is_some_and(|r| r.starts_with('/')))
            .expect("/ is always mounted");
        let inner = if m.at == "/" { path } else { &path[m.at.len()..] };
        let inner = match inner.trim_start_matches('/') {
            "" => ".",
            p => p,
        };
        (m, inner)
    }

    /// Resolve the symbolic links in a VM path, as the VM's name space sees them: a relative
    /// target is taken from the link's directory, an absolute one from the VM's `/` (not the
    /// host's), across mounts. The last component is followed only if `follow_last`. More than
    /// 40 links is `eloop`. The result names no links (save the last, if not followed), so
    /// cap-std never has to follow one; it still refuses anything that would leave a mount.
    fn walk(&self, path: &str, follow_last: bool) -> Result<String, FileError> {
        let mut todo: std::collections::VecDeque<String> = path.split('/').filter(|c| !c.is_empty()).map(String::from).collect();
        let mut done: Vec<String> = Vec::new();
        let mut links = 0;
        while let Some(c) = todo.pop_front() {
            match c.as_str() {
                "." => continue,
                ".." => {
                    done.pop();
                    continue;
                }
                _ => {}
            }
            done.push(c);
            if todo.is_empty() && !follow_last {
                break;
            }
            let here = format!("/{}", done.join("/"));
            let (m, inner) = self.at(&here);
            let Ok(meta) = m.dir.symlink_metadata(inner) else { continue };
            if !meta.file_type().is_symlink() {
                continue;
            }
            links += 1;
            if links > 40 {
                return Err(FileError::Eloop);
            }
            let target = m.dir.read_link_contents(inner).map_err(error)?;
            let target = target.to_str().ok_or(FileError::Einval)?;
            done.pop();
            if target.starts_with('/') {
                done.clear();
            }
            for (i, part) in target.split('/').filter(|p| !p.is_empty()).enumerate() {
                todo.insert(i, String::from(part));
            }
        }
        Ok(format!("/{}", done.join("/")))
    }

    /// Like [`HostDir::at`], for an operation that changes something.
    fn at_writable<'p>(&self, path: &'p str) -> Result<(&Dir, &'p str), FileError> {
        let (m, inner) = self.at(path);
        if m.read_only {
            return Err(FileError::Erofs);
        }
        Ok((&m.dir, inner))
    }

    fn file(&mut self, handle: u64) -> Result<&mut std::fs::File, FileError> {
        self.open.get_mut(&handle).ok_or(FileError::Ebadf)
    }
}

fn error(e: std::io::Error) -> FileError {
    match e.kind() {
        ErrorKind::NotFound => FileError::Enoent,
        ErrorKind::PermissionDenied => FileError::Eacces,
        ErrorKind::AlreadyExists => FileError::Eexist,
        ErrorKind::NotADirectory => FileError::Enotdir,
        ErrorKind::IsADirectory => FileError::Eisdir,
        ErrorKind::DirectoryNotEmpty => FileError::Enotempty,
        ErrorKind::ReadOnlyFilesystem => FileError::Erofs,
        ErrorKind::StorageFull => FileError::Enospc,
        ErrorKind::CrossesDevices => FileError::Exdev,
        ErrorKind::InvalidFilename => FileError::Enametoolong,
        ErrorKind::InvalidInput => FileError::Einval,
        ErrorKind::Unsupported => FileError::Enotsup,
        _ => FileError::Eio,
    }
}

fn info(m: &impl MetadataLike) -> FileInfo {
    let mode = m.mode();
    let kind = match mode & 0o170000 {
        0o100000 => FileKind::Regular,
        0o040000 => FileKind::Directory,
        0o120000 => FileKind::Symlink,
        _ => FileKind::Other,
    };
    FileInfo {
        size: m.size(),
        kind,
        // From the permission bits for the owner: good enough for a test platform.
        readable: mode & 0o400 != 0,
        writable: mode & 0o200 != 0,
        atime: m.atime(),
        mtime: m.mtime(),
        ctime: m.ctime(),
        mode,
        links: m.nlink(),
        inode: m.ino(),
        uid: m.uid(),
        gid: m.gid(),
    }
}

/// `stat` fields, from either cap-std's or std's metadata.
trait MetadataLike {
    fn mode(&self) -> u32;
    fn size(&self) -> u64;
    fn atime(&self) -> i64;
    fn mtime(&self) -> i64;
    fn ctime(&self) -> i64;
    fn nlink(&self) -> u64;
    fn ino(&self) -> u64;
    fn uid(&self) -> u32;
    fn gid(&self) -> u32;
}

macro_rules! metadata_like {
    ($t:ty, $ext:path) => {
        impl MetadataLike for $t {
            fn mode(&self) -> u32 {
                <$t as $ext>::mode(self)
            }
            fn size(&self) -> u64 {
                <$t as $ext>::size(self)
            }
            fn atime(&self) -> i64 {
                <$t as $ext>::atime(self)
            }
            fn mtime(&self) -> i64 {
                <$t as $ext>::mtime(self)
            }
            fn ctime(&self) -> i64 {
                <$t as $ext>::ctime(self)
            }
            fn nlink(&self) -> u64 {
                <$t as $ext>::nlink(self)
            }
            fn ino(&self) -> u64 {
                <$t as $ext>::ino(self)
            }
            fn uid(&self) -> u32 {
                <$t as $ext>::uid(self)
            }
            fn gid(&self) -> u32 {
                <$t as $ext>::gid(self)
            }
        }
    };
}
metadata_like!(cap_std::fs::Metadata, MetadataExt);
metadata_like!(std::fs::Metadata, std::os::unix::fs::MetadataExt);

impl Files for HostDir {
    fn open(&mut self, path: &str, mode: OpenMode) -> Result<u64, FileError> {
        let mut o = OpenOptions::new();
        o.read(mode.read).write(mode.write && !mode.append).append(mode.append).truncate(mode.truncate);
        if mode.exclusive {
            o.create_new(true);
        } else {
            o.create(mode.create);
        }
        let path = self.walk(path, true)?;
        let (m, inner) = self.at(&path);
        if m.read_only && (mode.write || mode.create || mode.truncate || mode.append) {
            return Err(FileError::Erofs);
        }
        let f = m.dir.open_with(inner, &o).map_err(error)?.into_std();
        let h = self.next;
        self.next += 1;
        self.open.insert(h, f);
        Ok(h)
    }

    fn close(&mut self, handle: u64) {
        self.open.remove(&handle);
    }

    fn read(&mut self, handle: u64, len: usize) -> Result<Vec<u8>, FileError> {
        let mut buf = vec![0; len];
        let n = self.file(handle)?.read(&mut buf).map_err(error)?;
        buf.truncate(n);
        Ok(buf)
    }

    fn write(&mut self, handle: u64, data: &[u8]) -> Result<(), FileError> {
        self.file(handle)?.write_all(data).map_err(error)
    }

    fn pread(&mut self, handle: u64, offset: u64, len: usize) -> Result<Vec<u8>, FileError> {
        let mut buf = vec![0; len];
        let n = self.file(handle)?.read_at(&mut buf, offset).map_err(error)?;
        buf.truncate(n);
        Ok(buf)
    }

    fn pwrite(&mut self, handle: u64, offset: u64, data: &[u8]) -> Result<(), FileError> {
        self.file(handle)?.write_all_at(data, offset).map_err(error)
    }

    fn seek(&mut self, handle: u64, to: SeekFrom) -> Result<u64, FileError> {
        let to = match to {
            SeekFrom::Start(n) => std::io::SeekFrom::Start(n),
            SeekFrom::Current(n) => std::io::SeekFrom::Current(n),
            SeekFrom::End(n) => std::io::SeekFrom::End(n),
        };
        self.file(handle)?.seek(to).map_err(error)
    }

    fn truncate(&mut self, handle: u64) -> Result<(), FileError> {
        let f = self.file(handle)?;
        let pos = f.stream_position().map_err(error)?;
        f.set_len(pos).map_err(error)
    }

    fn sync(&mut self, handle: u64) -> Result<(), FileError> {
        self.file(handle)?.sync_all().map_err(error)
    }

    fn handle_info(&mut self, handle: u64) -> Result<FileInfo, FileError> {
        Ok(info(&self.file(handle)?.metadata().map_err(error)?))
    }

    fn info(&mut self, path: &str, follow: bool) -> Result<FileInfo, FileError> {
        let path = self.walk(path, follow)?;
        let (m, inner) = self.at(&path);
        let m = m.dir.symlink_metadata(inner);
        Ok(info(&m.map_err(error)?))
    }

    fn list_dir(&mut self, path: &str) -> Result<Vec<Vec<u8>>, FileError> {
        let path = self.walk(path, true)?;
        let path = path.as_str();
        let (m, inner) = self.at(path);
        let mut names = Vec::new();
        for entry in m.dir.read_dir(inner).map_err(error)? {
            names.push(entry.map_err(error)?.file_name().as_bytes().to_vec());
        }
        // Mount points directly inside this directory appear in it.
        let prefix = if path == "/" { String::from("/") } else { format!("{path}/") };
        for mount in &self.mounts {
            if let Some(name) = mount.at.strip_prefix(prefix.as_str()).filter(|n| !n.is_empty() && !n.contains('/')) {
                if !names.iter().any(|n| n == name.as_bytes()) {
                    names.push(name.as_bytes().to_vec());
                }
            }
        }
        Ok(names)
    }

    fn make_dir(&mut self, path: &str) -> Result<(), FileError> {
        let path = self.walk(path, false)?;
        let (d, inner) = self.at_writable(&path)?;
        d.create_dir(inner).map_err(error)
    }

    fn delete(&mut self, path: &str) -> Result<(), FileError> {
        let path = self.walk(path, false)?;
        let (d, inner) = self.at_writable(&path)?;
        d.remove_file(inner).map_err(error)
    }

    fn del_dir(&mut self, path: &str) -> Result<(), FileError> {
        let path = self.walk(path, false)?;
        let (d, inner) = self.at_writable(&path)?;
        d.remove_dir(inner).map_err(error)
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), FileError> {
        let (from, to) = (self.walk(from, false)?, self.walk(to, false)?);
        let ((a, from), (b, to)) = (self.at_writable(&from)?, self.at_writable(&to)?);
        if !std::ptr::eq(a, b) {
            return Err(FileError::Exdev);
        }
        a.rename(from, b, to).map_err(error)
    }

    fn set_times(&mut self, path: &str, atime: i64, mtime: i64) -> Result<(), FileError> {
        let at = |secs: i64| std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(secs.max(0) as u64));
        let (Some(a), Some(m)) = (at(atime), at(mtime)) else { return Err(FileError::Einval) };
        // By host path, which works whatever the file's mode (opening it first would not); the
        // path names no links, having been walked.
        let path = self.walk(path, true)?;
        let (mount, inner) = self.at(&path);
        if mount.read_only {
            return Err(FileError::Erofs);
        }
        use fs_set_times::SystemTimeSpec::Absolute;
        fs_set_times::set_times(mount.host.join(inner), Some(Absolute(a)), Some(Absolute(m))).map_err(error)
    }

    fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), FileError> {
        // By host path, which (unlike opening the file first) works whatever its mode; the
        // path names no links, having been walked.
        let path = self.walk(path, true)?;
        let (m, inner) = self.at(&path);
        if m.read_only {
            return Err(FileError::Erofs);
        }
        std::fs::set_permissions(m.host.join(inner), std::os::unix::fs::PermissionsExt::from_mode(mode)).map_err(error)
    }

    fn make_symlink(&mut self, target: &[u8], link: &str) -> Result<(), FileError> {
        let target = std::path::Path::new(std::ffi::OsStr::from_bytes(target));
        // The target is stored as given; `walk` interprets it within the VM's name space.
        let link = self.walk(link, false)?;
        let (d, inner) = self.at_writable(&link)?;
        d.symlink_contents(target, inner).map_err(error)
    }

    fn make_link(&mut self, existing: &str, new: &str) -> Result<(), FileError> {
        let (existing, new) = (self.walk(existing, false)?, self.walk(new, false)?);
        let ((a, existing), (b, new)) = (self.at_writable(&existing)?, self.at_writable(&new)?);
        if !std::ptr::eq(a, b) {
            return Err(FileError::Exdev);
        }
        a.hard_link(existing, b, new).map_err(error)
    }

    fn read_link(&mut self, path: &str) -> Result<Vec<u8>, FileError> {
        let path = self.walk(path, false)?;
        let (m, inner) = self.at(&path);
        let target = m.dir.read_link_contents(inner).map_err(error)?;
        Ok(target.as_os_str().as_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temp dir, removed when dropped.
    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let p = std::env::temp_dir().join(format!("beamlet-files-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(p.join("root")).unwrap();
            Scratch(p)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn symlinks_cannot_leave_the_root() {
        let s = Scratch::new("escape");
        std::fs::write(s.0.join("secret"), b"outside").unwrap();
        std::os::unix::fs::symlink(s.0.join("secret"), s.0.join("root/abs")).unwrap();
        std::os::unix::fs::symlink("../secret", s.0.join("root/rel")).unwrap();
        std::os::unix::fs::symlink("..", s.0.join("root/up")).unwrap();
        let mut fs = HostDir::new(s.0.join("root").to_str().unwrap()).unwrap();
        let read = OpenMode { read: true, ..OpenMode::default() };
        // Links are followed within the VM's name space: none of them reaches the secret.
        for p in ["/abs", "/rel", "/up/secret"] {
            assert!(fs.open(p, read).is_err(), "{p}");
            assert!(fs.info(p, true).is_err(), "{p}");
        }
        // An absolute target means the VM's `/`.
        std::fs::write(s.0.join("root/inside"), b"in").unwrap();
        fs.make_symlink(b"/inside", "/abs_in").unwrap();
        let h = fs.open("/abs_in", read).unwrap();
        assert_eq!(fs.read(h, 10).unwrap(), b"in");
        assert_eq!(fs.read_link("/abs_in").unwrap(), b"/inside");
        // A cycle is eloop.
        fs.make_symlink(b"/cyc2", "/cyc1").unwrap();
        fs.make_symlink(b"/cyc1", "/cyc2").unwrap();
        assert_eq!(fs.open("/cyc1", read), Err(FileError::Eloop));
        // The link itself is inside and may be inspected.
        assert_eq!(fs.info("/abs", false).unwrap().kind, FileKind::Symlink);
    }

    #[test]
    fn mounts_are_separate_and_may_be_read_only() {
        let s = Scratch::new("mount");
        std::fs::create_dir_all(s.0.join("lib/kernel")).unwrap();
        std::fs::write(s.0.join("lib/kernel/k.hrl"), b"x").unwrap();
        let mut fs = HostDir::new(s.0.join("root").to_str().unwrap()).unwrap();
        fs.mount("/otp", s.0.join("lib").to_str().unwrap(), true).unwrap();
        let read = OpenMode { read: true, ..OpenMode::default() };
        let h = fs.open("/otp/kernel/k.hrl", read).unwrap();
        assert_eq!(fs.read(h, 10).unwrap(), b"x");
        let w = OpenMode { write: true, create: true, truncate: true, ..OpenMode::default() };
        assert_eq!(fs.open("/otp/kernel/new", w), Err(FileError::Erofs));
        assert_eq!(fs.make_dir("/otp/d"), Err(FileError::Erofs));
        assert!(fs.list_dir("/").unwrap().contains(&b"otp".to_vec()));
        assert_eq!(fs.list_dir("/otp").unwrap(), vec![b"kernel".to_vec()]);
        // `/otpx` is not inside the `/otp` mount.
        assert_eq!(fs.info("/otpx", true).unwrap_err(), FileError::Enoent);
        fs.make_dir("/d").unwrap();
        assert_eq!(fs.rename("/d", "/otp/d"), Err(FileError::Erofs));
    }

    #[test]
    fn files_round_trip() {
        let s = Scratch::new("rt");
        let mut fs = HostDir::new(s.0.join("root").to_str().unwrap()).unwrap();
        fs.make_dir("/d").unwrap();
        let w = OpenMode { write: true, create: true, truncate: true, ..OpenMode::default() };
        let h = fs.open("/d/f", w).unwrap();
        fs.write(h, b"hello world").unwrap();
        fs.close(h);
        let h = fs.open("/d/f", OpenMode { read: true, ..OpenMode::default() }).unwrap();
        assert_eq!(fs.pread(h, 6, 100).unwrap(), b"world");
        assert_eq!(fs.read(h, 5).unwrap(), b"hello");
        assert_eq!(fs.seek(h, SeekFrom::End(-1)).unwrap(), 10);
        fs.close(h);
        assert_eq!(fs.read(h, 1), Err(FileError::Ebadf));
        assert_eq!(fs.list_dir("/d").unwrap(), vec![b"f".to_vec()]);
        assert_eq!(fs.del_dir("/d"), Err(FileError::Enotempty));
        fs.rename("/d/f", "/g").unwrap();
        fs.del_dir("/d").unwrap();
        assert_eq!(fs.info("/g", true).unwrap().size, 11);
        let excl = OpenMode { write: true, create: true, exclusive: true, ..OpenMode::default() };
        assert_eq!(fs.open("/g", excl), Err(FileError::Eexist));
    }
}
