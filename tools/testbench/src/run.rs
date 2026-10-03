//! A bench run's own directory (docs/testbench.md, "How to use it"). Runs may overlap in one
//! worktree, so each writes its files under `target/testbench/run-<pid>-<time>/`, never at a path
//! another run writes; `target/testbench/last` names the latest for people. The time is in the
//! name because a pid is not enough: each dev container has its own pid namespace, where every
//! bench starts with much the same pid.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

/// How many of the newest runs are kept, besides the live ones, when a run starts.
const KEEP: usize = 8;

pub struct Run {
    pub dir: PathBuf,
    /// Held for the run's life, so that no other run prunes this one (`flock`: released by the
    /// kernel if the run dies).
    _lock: File,
}

impl Run {
    /// Make this run's directory under `root`, lock it, point `root/last` at it, and remove the
    /// runs older than the `KEEP` newest that no live run holds.
    pub fn start(root: &Path) -> Result<Run> {
        std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
        let (name, dir) = loop {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let name = format!("run-{}-{nanos}", std::process::id());
            let dir = root.join(&name);
            match std::fs::create_dir(&dir) {
                Ok(()) => break (name, dir),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).with_context(|| format!("creating {}", dir.display())),
            }
        };
        let lock = File::create(dir.join("lock"))?;
        lock.lock()?;
        // A new link renamed over the old one: a reader sees one run or the other, never neither.
        let link = root.join(format!(".{name}.last"));
        std::os::unix::fs::symlink(&name, &link)?;
        std::fs::rename(&link, root.join("last"))?;
        prune(root, KEEP)?;
        Ok(Run { dir, _lock: lock })
    }
}

/// Remove every run under `root` older than the `keep` newest, unless a live run holds its lock.
fn prune(root: &Path, keep: usize) -> Result<()> {
    let mut runs: Vec<(u128, PathBuf)> = std::fs::read_dir(root)?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            let started = name.strip_prefix("run-")?.rsplit_once('-')?.1.parse().ok()?;
            Some((started, path))
        })
        .collect();
    runs.sort_unstable_by_key(|run| std::cmp::Reverse(run.0));
    for (_, dir) in runs.into_iter().skip(keep) {
        // A run that has not made its lock yet is among the newest, never here, while fewer than
        // `KEEP` runs start at once.
        let Ok(lock) = File::open(dir.join("lock")) else { continue };
        if lock.try_lock().is_ok() {
            // Another run starting now may be removing it too.
            match std::fs::remove_dir_all(&dir) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(e).with_context(|| format!("removing {}", dir.display()));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two runs started together, as in two containers whose benches have the same pid, get
    /// directories of their own; `last` names the latest; pruning removes an old run only once
    /// no live run holds it.
    #[test]
    fn overlapping_runs_get_their_own_directories() {
        let root = std::env::temp_dir().join(format!("testbench-runs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let first = Run::start(&root).unwrap();
        let second = Run::start(&root).unwrap();
        assert_ne!(first.dir, second.dir);
        assert_eq!(
            std::fs::canonicalize(root.join("last")).unwrap(),
            std::fs::canonicalize(&second.dir).unwrap()
        );

        prune(&root, 0).unwrap();
        assert!(first.dir.is_dir() && second.dir.is_dir(), "a live run was pruned");
        // The first run ends. Unlocked rather than closed: a child another test forks in this
        // process shares the lock's file until it execs, and closing alone leaves it held.
        first._lock.unlock().unwrap();
        let first_dir = first.dir.clone();
        prune(&root, 1).unwrap();
        assert!(!first_dir.exists(), "an old run nobody holds was kept");
        assert!(second.dir.is_dir(), "the newest run was pruned");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
