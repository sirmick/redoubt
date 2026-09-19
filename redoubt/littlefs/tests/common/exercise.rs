//! Drives every operation against a possibly hostile volume, ignoring errors: the only
//! requirement is that nothing panics or hangs. Shared by `tests/hostile.rs` and the fuzzer
//! (which includes this file by path).

use littlefs::{BlockDevice, FileType, Filesystem, OpenOptions};

/// Walks at most `limit` directories (a hostile tree may loop), reading everything.
fn walk<D: BlockDevice>(fs: &mut Filesystem<D>, limit: usize) {
    let mut todo = vec![String::from("/")];
    let mut visited = 0;
    while let Some(dir) = todo.pop() {
        visited += 1;
        if visited > limit {
            return;
        }
        let mut entries = Vec::new();
        let _ = fs.read_dir(&dir, |e| entries.push((e.name.to_vec(), e.meta.kind)));
        let _ = fs.get_attr(&dir, 1);
        for (name, kind) in entries {
            let Ok(name) = String::from_utf8(name) else { continue };
            let path = format!("{}/{name}", dir.trim_end_matches('/'));
            let _ = fs.stat(&path);
            let _ = fs.get_attr(&path, 0x74);
            match kind {
                FileType::Dir => todo.push(path),
                FileType::File => {
                    if let Ok(h) = fs.open(&path, OpenOptions { read: true, ..Default::default() }) {
                        let mut buf = vec![0u8; 4096];
                        for _ in 0..16 {
                            if !matches!(fs.read(h, &mut buf), Ok(n) if n > 0) {
                                break;
                            }
                        }
                        let end = fs.file_size(h).unwrap_or(0);
                        let _ = fs.seek(h, end.saturating_sub(1));
                        let _ = fs.read(h, &mut buf);
                        let _ = fs.close(h);
                    }
                }
            }
        }
    }
}

pub fn exercise<D: BlockDevice>(fs: &mut Filesystem<D>) {
    walk(fs, 64);
    let _ = fs.fsck();
    let _ = fs.mkdir("/fz");
    let w = OpenOptions { read: true, write: true, create: true, ..Default::default() };
    if let Ok(h) = fs.open("/fz/f", w) {
        let _ = fs.write(h, &[0x5a; 3000]);
        let _ = fs.seek(h, 100);
        let _ = fs.write(h, b"patch");
        let _ = fs.truncate(h, 1500);
        let _ = fs.close(h);
    }
    let _ = fs.set_attr("/fz/f", 1, b"attr");
    let _ = fs.rename("/fz/f", "/moved");
    let _ = fs.set_attr("/", 2, b"root");
    walk(fs, 64);
    let _ = fs.remove("/moved");
    let _ = fs.remove("/fz");
    let _ = fs.fsck();
}
