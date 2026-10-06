//! Drives a volume that may be hostile: a walk that reads each file only so far, then the check
//! and a change of every kind. Shared by the hostile tests and the fuzz targets; only a panic or a
//! hang is a failure.

use walfs::{BlockDevice, Filesystem, OpenOptions};

use super::{Node, walk};

/// The most of one file a walk of a hostile volume reads.
pub const READ_CAP: usize = 64 * 1024;

/// Walks, checks and changes the volume; every outcome is allowed but a panic or a hang.
pub fn exercise<D: BlockDevice>(fs: &mut Filesystem<D>) {
    let found = walk(fs, READ_CAP);
    let _ = fs.check();
    let rw = OpenOptions { read: true, write: true, create: true, ..Default::default() };
    if let Ok(h) = fs.open("/fz", rw) {
        let _ = fs.write(h, &[0x5a; 9000]);
        let _ = fs.seek(h, 4_300_000);
        let _ = fs.write(h, b"far");
        let _ = fs.truncate(h, 100);
        let _ = fs.close(h);
    }
    let _ = fs.mkdir("/fz-d");
    let _ = fs.rename("/fz", "/fz-d/moved");
    let _ = fs.set_attr("/fz-d", 7, b"fuzz");
    // Every file the walk found: written in the middle, cut, renamed and removed.
    for (path, node) in found.iter().flatten().take(16) {
        if path.is_empty() {
            continue;
        }
        if let Node::File { .. } = node {
            if let Ok(h) = fs.open(path, OpenOptions { write: true, ..Default::default() }) {
                let _ = fs.seek(h, 3);
                let _ = fs.write(h, b"changed");
                let _ = fs.truncate(h, 2);
                let _ = fs.close(h);
            }
        }
        let _ = fs.rename(path, &format!("{path}-r"));
        let _ = fs.remove(&format!("{path}-r"));
    }
    let _ = fs.remove("/fz-d/moved");
    let _ = fs.remove("/fz-d");
    let _ = walk(fs, READ_CAP);
    let _ = fs.check();
}
