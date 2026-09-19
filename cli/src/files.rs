//! The POSIX file system for a VM: one host directory, exposed as the VM's `/`.
//!
//! Every path is opened relative to that directory through `cap-std`, which refuses anything
//! that would leave it: `..` past the top (the VM resolves those already) and, what the VM
//! cannot see, symbolic links pointing outside. An escape attempt fails with `eacces`.

use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Seek, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileExt;

use beamlet_vm::platform::{FileError, FileInfo, FileKind, Files, OpenMode, SeekFrom};
use cap_std::fs::{Dir, MetadataExt, OpenOptions};

pub struct HostDir {
    root: Dir,
    open: BTreeMap<u64, std::fs::File>,
    next: u64,
}

impl HostDir {
    pub fn new(dir: &str) -> std::io::Result<HostDir> {
        let root = Dir::open_ambient_dir(dir, cap_std::ambient_authority())?;
        Ok(HostDir { root, open: BTreeMap::new(), next: 1 })
    }

    fn file(&mut self, handle: u64) -> Result<&mut std::fs::File, FileError> {
        self.open.get_mut(&handle).ok_or(FileError::Ebadf)
    }
}

/// A VM path (`/a/b`, already normalized) as a path relative to the root.
fn rel(path: &str) -> &str {
    match path.trim_start_matches('/') {
        "" => ".",
        p => p,
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
        let f = self.root.open_with(rel(path), &o).map_err(error)?.into_std();
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
        let m = if follow { self.root.metadata(rel(path)) } else { self.root.symlink_metadata(rel(path)) };
        Ok(info(&m.map_err(error)?))
    }

    fn list_dir(&mut self, path: &str) -> Result<Vec<Vec<u8>>, FileError> {
        let mut names = Vec::new();
        for entry in self.root.read_dir(rel(path)).map_err(error)? {
            names.push(entry.map_err(error)?.file_name().as_bytes().to_vec());
        }
        Ok(names)
    }

    fn make_dir(&mut self, path: &str) -> Result<(), FileError> {
        self.root.create_dir(rel(path)).map_err(error)
    }

    fn delete(&mut self, path: &str) -> Result<(), FileError> {
        self.root.remove_file(rel(path)).map_err(error)
    }

    fn del_dir(&mut self, path: &str) -> Result<(), FileError> {
        self.root.remove_dir(rel(path)).map_err(error)
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), FileError> {
        self.root.rename(rel(from), &self.root, rel(to)).map_err(error)
    }

    fn read_link(&mut self, path: &str) -> Result<Vec<u8>, FileError> {
        let target = self.root.read_link(rel(path)).map_err(error)?;
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
        for p in ["/abs", "/rel", "/up/secret"] {
            assert_eq!(fs.open(p, read), Err(FileError::Eacces), "{p}");
            assert!(fs.info(p, true).is_err(), "{p}");
        }
        // The link itself is inside and may be inspected.
        assert_eq!(fs.info("/abs", false).unwrap().kind, FileKind::Symlink);
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
