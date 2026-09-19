//! A safe wrapper around the littlefs C reference, over a RAM image, for differential tests.
//! Host-only test code: the FFI below is the only `unsafe` in the littlefs work, and it never
//! runs on the machine.

use std::ffi::CString;

#[repr(C)]
struct Shim {
    _private: [u8; 0],
}

extern "C" {
    fn shim_new(image: *mut u8, bs: u32, count: u32, prog: u32, cycles: i32) -> *mut Shim;
    fn shim_free(s: *mut Shim);
    fn shim_format(s: *mut Shim) -> i32;
    fn shim_mount(s: *mut Shim) -> i32;
    fn shim_unmount(s: *mut Shim) -> i32;
    fn shim_mkdir(s: *mut Shim, path: *const i8) -> i32;
    fn shim_remove(s: *mut Shim, path: *const i8) -> i32;
    fn shim_rename(s: *mut Shim, from: *const i8, to: *const i8) -> i32;
    fn shim_setattr(s: *mut Shim, path: *const i8, typ: u8, data: *const u8, len: u32) -> i32;
    fn shim_removeattr(s: *mut Shim, path: *const i8, typ: u8) -> i32;
    fn shim_getattr(s: *mut Shim, path: *const i8, typ: u8, buf: *mut u8, cap: u32) -> i32;
    fn shim_write(
        s: *mut Shim,
        path: *const i8,
        create: i32,
        trunc: i32,
        at: u32,
        data: *const u8,
        len: u32,
        cut: i64,
    ) -> i32;
    fn shim_read(s: *mut Shim, path: *const i8, buf: *mut u8, cap: u32) -> i32;
    fn shim_list(s: *mut Shim, path: *const i8, out: *mut u8, cap: u32) -> i32;
}

/// The reference's error codes are negative integers.
pub type CResult<T> = Result<T, i32>;

pub const LFS_ERR_NOATTR: i32 = -61;
pub const LFS_TYPE_DIR: u8 = 2;

fn check(r: i32) -> CResult<()> { if r < 0 { Err(r) } else { Ok(()) } }

fn c(path: &str) -> CString { CString::new(path).expect("test paths have no NUL") }

/// A mounted C littlefs over an image it holds; `into_image` unmounts and returns it.
pub struct CFs {
    shim: *mut Shim,
    // Boxed so its address is stable: the C side keeps a pointer to it.
    image: Box<[u8]>,
    mounted: bool,
}

/// C configuration beyond the geometry.
#[derive(Clone, Copy)]
pub struct CConfig {
    pub block_size: u32,
    pub block_count: u32,
    pub prog_size: u32,
    /// Wear levelling: metadata relocation every this many erases (-1: off).
    pub block_cycles: i32,
}

impl CFs {
    fn new(cfg: CConfig, image: Vec<u8>) -> CFs {
        assert_eq!(image.len(), (cfg.block_size * cfg.block_count) as usize);
        let mut image = image.into_boxed_slice();
        // SAFETY: `image` is a live allocation of block_size * block_count bytes that `CFs`
        // owns and never moves or resizes until `drop`, which frees the shim first; the shim
        // only reads and writes inside it.
        let shim = unsafe {
            shim_new(
                image.as_mut_ptr(),
                cfg.block_size,
                cfg.block_count,
                cfg.prog_size,
                cfg.block_cycles,
            )
        };
        assert!(!shim.is_null());
        CFs { shim, image, mounted: false }
    }

    pub fn format(cfg: CConfig) -> CFs {
        let fs = CFs::new(cfg, vec![0xff; (cfg.block_size * cfg.block_count) as usize]);
        // SAFETY: `fs.shim` is the live shim `new` returned.
        check(unsafe { shim_format(fs.shim) }).expect("C format");
        fs
    }

    pub fn mount(cfg: CConfig, image: Vec<u8>) -> CResult<CFs> {
        let mut fs = CFs::new(cfg, image);
        // SAFETY: `fs.shim` is the live shim `new` returned.
        check(unsafe { shim_mount(fs.shim) })?;
        fs.mounted = true;
        Ok(fs)
    }

    pub fn into_image(mut self) -> Vec<u8> {
        if self.mounted {
            // SAFETY: the shim is live and mounted.
            unsafe { shim_unmount(self.shim) };
            self.mounted = false;
        }
        std::mem::take(&mut self.image).into_vec()
    }

    pub fn mkdir(&mut self, path: &str) -> CResult<()> {
        let p = c(path);
        // SAFETY: live mounted shim; `p` is a NUL-terminated string that outlives the call.
        check(unsafe { shim_mkdir(self.shim, p.as_ptr()) })
    }

    pub fn remove(&mut self, path: &str) -> CResult<()> {
        let p = c(path);
        // SAFETY: as in `mkdir`.
        check(unsafe { shim_remove(self.shim, p.as_ptr()) })
    }

    pub fn rename(&mut self, from: &str, to: &str) -> CResult<()> {
        let (a, b) = (c(from), c(to));
        // SAFETY: as in `mkdir`, for both strings.
        check(unsafe { shim_rename(self.shim, a.as_ptr(), b.as_ptr()) })
    }

    pub fn set_attr(&mut self, path: &str, typ: u8, data: &[u8]) -> CResult<()> {
        let p = c(path);
        // SAFETY: as in `mkdir`; `data` is valid for `data.len()` bytes during the call.
        check(unsafe { shim_setattr(self.shim, p.as_ptr(), typ, data.as_ptr(), data.len() as u32) })
    }

    pub fn remove_attr(&mut self, path: &str, typ: u8) -> CResult<()> {
        let p = c(path);
        // SAFETY: as in `mkdir`.
        check(unsafe { shim_removeattr(self.shim, p.as_ptr(), typ) })
    }

    pub fn get_attr(&mut self, path: &str, typ: u8) -> CResult<Vec<u8>> {
        let p = c(path);
        let mut buf = vec![0u8; 1024];
        // SAFETY: as in `mkdir`; `buf` is writable for the capacity passed.
        let n = unsafe { shim_getattr(self.shim, p.as_ptr(), typ, buf.as_mut_ptr(), buf.len() as u32) };
        check(n)?;
        buf.truncate(n as usize);
        Ok(buf)
    }

    /// Writes `data` at `at` (after emptying the file if `trunc`), then truncates to `cut`.
    pub fn write(&mut self, path: &str, create: bool, trunc: bool, at: u32, data: &[u8], cut: Option<u32>) -> CResult<()> {
        let p = c(path);
        let cut = cut.map_or(-1, i64::from);
        // SAFETY: as in `mkdir`; `data` is readable for `data.len()` bytes during the call.
        check(unsafe {
            shim_write(self.shim, p.as_ptr(), create as i32, trunc as i32, at, data.as_ptr(), data.len() as u32, cut)
        })
    }

    pub fn read(&mut self, path: &str) -> CResult<Vec<u8>> {
        let p = c(path);
        let mut buf = vec![0u8; 1 << 20];
        // SAFETY: as in `mkdir`; `buf` is writable for the capacity passed.
        let n = unsafe { shim_read(self.shim, p.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        check(n)?;
        assert!((n as usize) <= buf.len(), "test files stay under 1 MiB");
        buf.truncate(n as usize);
        Ok(buf)
    }

    /// Entries of a directory: (is_dir, size, name).
    pub fn list(&mut self, path: &str) -> CResult<Vec<(bool, u32, String)>> {
        let p = c(path);
        let mut out = vec![0u8; 1 << 20];
        // SAFETY: as in `mkdir`; `out` is writable for the capacity passed.
        let n = unsafe { shim_list(self.shim, p.as_ptr(), out.as_mut_ptr(), out.len() as u32) };
        check(n)?;
        let mut entries = Vec::new();
        let mut i = 0;
        while i < n as usize {
            let typ = out[i];
            let size = u32::from_le_bytes(out[i + 1..i + 5].try_into().unwrap());
            let len = u16::from_le_bytes([out[i + 5], out[i + 6]]) as usize;
            let name = String::from_utf8(out[i + 7..i + 7 + len].to_vec()).unwrap();
            entries.push((typ == LFS_TYPE_DIR, size, name));
            i += 7 + len;
        }
        Ok(entries)
    }
}

impl Drop for CFs {
    fn drop(&mut self) {
        if self.mounted {
            // SAFETY: the shim is live and mounted.
            unsafe { shim_unmount(self.shim) };
        }
        // SAFETY: `shim` came from `shim_new` and is freed exactly once, here, while `image`
        // (which it points into) is still alive.
        unsafe { shim_free(self.shim) };
    }
}
