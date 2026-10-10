# TRY2 report (try2-implementer, 2026-10-09)

Branch wp-TRY2 in /home/mcloonan/redoubt/.worktrees/TRY2. Base main f5316e38c. Head 8eef794b2,
one commit (28657f06c rebased and amended). Not pushed.

## 1. 28657f06c against the node body

| Node body | As built |
| --- | --- |
| `--keep-disk` keeps the disk image under .tmp per arch | `$REDOUBT_TMP/launch/disk-<arch>.img` (launch.rs `launch_dir`) |
| reuses it if its layout matches; else refuses with the reason and the flag, never reformats | `qemu::check_layout`: size and GPT bytes against a fresh disk of `image/disk.toml`; the error names the reason, `--fresh-disk`, "not reformatted" |
| …matches the image's **manifest** | Gap, now fixed: a fresh pack held the manifest's volume `bytes` to the partitions (`disk::hold`); a kept disk skipped that. `disk::hold_recipe` now runs in both, with a new host test |
| without the flag, a fresh disk as now | unchanged path; launch's banner says so and names `--keep-disk` |
| read-only system volume from the build | it is the userland disk, re-copied from the run's pack at every launch beside the kept disk (`disk-<arch>.userland.img`) |
| GETTING-STARTED 'Try the system' says so | yes: the flags, the path, refusal, `--fresh-disk`, QEMU's lock, the case |
| bench case: write, relaunch, read back | `tests/launch-keep-disk.toml`: two boots of launch's line on one kept disk (`[disk] keep = true`), `x` then `xx` |

Also in the commit (in the message): the walk-through's `ed`/`fm` open /home/alice, since `/`
is not writable.

## 2. Rebase

`git rebase f5316e38c`: clean, no conflicts.

## Changes made in this round (amended into the one commit)

- tools/testbench/src/disk.rs: `hold_recipe` extracted from `pack`, used by `pack` and the kept path.
- tools/testbench/src/qemu.rs: `check_layout` calls it; test `a_kept_disk_is_held_to_its_recipe_s_manifest`; a doc comment reflowed.
- docs/testbench.md: status list (34) names the new test; the `launch` key's comment said "(one case)", now `(launch-*)`: two cases boot launch's line.
- Commit message: one sentence added on the manifest hold.

## 3. Gates (all env from launch-env-2026-10-09.md)

| Command | Result |
| --- | --- |
| `q run --cores 8 -- cargo test -p testbench kept_disk` | rc 0, 3 passed |
| `make -f scripts/jobs.mk prebuilt` | rc 0 (rv64 263, rv32 249 cases built) |
| `make -f scripts/jobs.mk set CASES="launch-keep-disk launch-system"` | rc 0: PASS launch-keep-disk rv64 smp4 9.5 s, rv32 smp4 8.8 s; launch-system rv64 5.4 s, rv32 5.0 s. Logs: first boot `boots: x`, second `boots: xx`, both widths |
| `make -f scripts/jobs.mk set CASES="host-tests"` (holds `testbench`) | rc 0, PASS 35.5 s (rv64; no rv32 target) |
| `make -f scripts/jobs.mk docs` | rc 0, at the head b5b8cb53a |
| `./launch --keep-disk` | rc 2, "need --system" |
| `./launch --system --fresh-disk --print-only` | rc 2, clap: needs `--keep-disk` |
| `./launch --system --keep-disk [--fresh-disk] --print-only` x3, REDOUBT_TMP=.tmp/TRY2/scratch | rc 0 each; banner: "new from the image", then "the one kept … as the last launch left it", then with --fresh-disk "new from the image" again |

Cases and host tests ran at 2a737946b; b5b8cb53a changes only a testbench.md comment line since
(docs rerun there). Not run: the whole bench, rv32 builds of the kernel (Tier B tooling; no
kernel, runtime or server code touched).

## Summaries checked

- GETTING-STARTED.md 'Try the system': updated in the commit (checked against code: path, refusal, lock, fresh-disk).
- docs/testbench.md "Disks and network cards" + case keys: updated (see above).
- `launch` usage header and `--help` range (lines 2-23): updated in the commit, checked.
- README.md, docs/TOUR.md, docs/plan/m2-usable-shell.md: grep for launch/disk/new at every launch: no claim about launch's disk; no change needed.
- tools/testbench/src/launch.rs module doc: updated in the commit.

## Round 2 (orchestrator 2026-10-09): the second launch's writes beside a running guest

Was: a second `--keep-disk` launch rewrote `disk-<arch>.userland.img` (and with `--fresh-disk`
removed the kept disk) beside a running guest, before QEMU's image lock refused it. Now:

- `launch` takes `flock -n` on `$REDOUBT_TMP/launch/disk-<arch>.lock` (fd 9) right after making the
  directory, before the build and any write, and exports REDOUBT_LAUNCH_DISK_LOCKED=1. fd 9 is held
  by the script and inherited by the exec'd QEMU, or by `q run`'s client, which lives as long as
  its job; `q` itself closes fds for the job, so the build cannot take it over, hence the variable.
- `launch.rs` `lock_kept`: without that variable the testbench takes the same lock itself
  (`File::try_lock`, flock(2), no `unsafe`) before `--fresh-disk`'s removal and before
  `command()` writes either image, held to the end of `system()` (its own QEMU, unless
  `--print-only`). The refusal: "the kept disk … is in use by another launch: quit it first".
- Host test `a_kept_disk_in_use_is_refused`: refused while held by `lock_kept`, and while held by
  flock(1) as `./launch` holds it (the same lock); taken again once let go; the disk unchanged.
- GETTING-STARTED's sentence and the commit message now say so (they named QEMU's lock).

Check by hand (REDOUBT_TMP=.tmp/TRY2/scratch, lock held by `flock -o … sleep 600`):
`./launch --system --keep-disk --print-only` rc 1, the same with `--fresh-disk` rc 1,
`cargo run -p testbench -- --run --system --keep-disk --print-only` rc 1, each with the message;
both kept images' mtimes unchanged (13:28:25); after the holder ended, the launch rc 0 ("the one
kept … as the last launch left it").

Gates at dce329295: `cargo test -p testbench kept_disk` 4 passed rc 0; prebuilt rc 0;
launch-keep-disk rv64 9.3 s / rv32 8.6 s PASS (x then xx on both); launch-system rv64 5.1 s /
rv32 4.8 s PASS; host-tests PASS 62.6 s; docs rc 0.

## Round 3 (steward-red OK with notes on dce329295): the two P3s

- **P3-1:** `lock_kept` now always tries the lock. A lock found held (WouldBlock) is taken as the
  caller's only when REDOUBT_LAUNCH_DISK_LOCKED is set. The variable alone, with the lock free, no
  longer skips it: the testbench takes the lock itself.
- **P3-2:** `./launch` writes `qemu-<arch>.$$.argv` (its own PID), reads it, then removes it, so a
  launch beside it cannot rewrite the line it boots.
- **Checked by hand** (REDOUBT_TMP=.tmp/TRY2/scratch, a stand-in `flock -o … sleep 600` holding the
  lock):

  | | rc |
  | --- | --- |
  | lock held: `./launch --keep-disk --print-only` | 1 |
  | lock held: testbench directly | 1 |
  | lock held: testbench directly with the variable set (the variable says the caller holds it) | 0 |
  | lock free: testbench with the variable set (takes the lock) | 0 |
  | lock free: `./launch` (the kept disk) | 0 |

  No per-launch argv file is left behind; the one `qemu-rv64.argv` in the directory is from before
  the change.
- **Gates at 8eef794b2:**
  - `cargo test -p testbench kept_disk`: 4 passed.
  - prebuilt: rc 0.
  - `launch-keep-disk`: rv64 9.5 s, rv32 8.1 s, PASS, `x` then `xx` on both.
  - `launch-system`: rv64 4.8 s, rv32 4.4 s, PASS.
  - host-tests: PASS, 62.2 s.
  - docs: rc 0.
- **Not tested:** a host test for the variable path, which would need the variable set in a shared
  test process.

## Residuals and risks

- `testbench --run --system --keep-disk --print-only` run by hand, without `./launch`, holds the
  lock only while it runs: QEMU booted later from its printed line is not covered. `./launch` is
  the interface that boots.
- The layout check compares the whole size and the GPT head only; a kept volume whose walfs format
  changed between builds is attached and walfsd judges it. The node asks for the layout only.
- `[disk] keep` in the bench needs `distinct_across_boots` (case.rs refuses it otherwise); each run
  dir is new (run.rs `create_dir`), so the first boot never finds a stale disk.

## Next

Review (steward-red, Tier B). Then B46 as assigned.
