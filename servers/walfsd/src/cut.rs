//! Test-only, for the bench's `walfsd-power-loss` (feature `cut-after-write`, off in every default
//! build): R50 on the machine. A walk to `walfsd-cut-N` arms a count of block writes; the
//! instance exits right after the N-th completes, inside whatever operation it was in, and
//! `init` restarts it. An exit after a completed device write leaves the medium as a power cut
//! after that write would: every write before it is on the disk and none after it is, and the
//! restarted instance's mount recovers the log as a power-on would.
//!
//! Every start under the feature also checks the volume whole once it mounts, and says what the
//! check found ([`said`]).

use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};

use walfs::{BlockDevice, Filesystem};

/// The walk that arms the cut: this, then the count in decimal.
pub const PROBE: &str = "walfsd-cut-";
/// What the instance exits with at the cut.
pub const CUT_EXIT: u32 = 10;

/// Block writes left before the cut; 0 when unarmed.
static LEFT: AtomicU32 = AtomicU32::new(0);

/// Arms the cut if `name` is the probe with a count of at least 1; says whether it was.
pub fn arm(name: &str) -> bool {
    let Some(n) = name.strip_prefix(PROBE).and_then(|n| n.parse::<u32>().ok()).filter(|n| *n > 0) else {
        return false;
    };
    LEFT.store(n, Ordering::Relaxed);
    true
}

/// A block write completed: at the armed count, the instance ends.
pub fn wrote() {
    if LEFT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if LEFT.fetch_sub(1, Ordering::Relaxed) == 1 {
        redoubt_rt::handle::process_exit(CUT_EXIT);
    }
}

/// The volume check's line: how many problems `check` found, the first of them, or the error
/// that stopped it.
pub fn said<D: BlockDevice>(fs: &mut Filesystem<D>) -> String {
    match fs.check() {
        Ok(problems) if problems.is_empty() => String::from("walfsd: the volume check found no problem\n"),
        Ok(problems) => {
            format!("walfsd: the volume check found {} problems: {:?}\n", problems.len(), problems[0])
        }
        Err(e) => format!("walfsd: the volume check stopped: {e:?}\n"),
    }
}
