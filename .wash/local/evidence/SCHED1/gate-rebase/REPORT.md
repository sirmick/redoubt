# SCHED1 rebase onto IPC3 410b19d75 and the short gate (impl-6, 2026-10-06)

## Branch

- `wp-SCHED1` tip **55af8fb8c**, made with `git rebase -i --autosquash --signoff --onto 410b19d75 4cc35d84b`.
  - The rebase folded the editor's two rewraps.
  - Every commit is signed off.
- Commits:
  1. `1c21c2ce5` testbench
  2. `bbfdf13b5` kernel
  3. `8250df8dd` tests
  4. `55af8fb8c` docs
- Nothing is pushed. The worktree is clean.

## Hunks

| File | Conflict | Resolution |
| --- | --- | --- |
| `tools/testbench/Cargo.toml` | main renamed fsd to littlefsd, added verity, client, sys and rt; this branch added `redoubt-stride` | kept main's block, added the `redoubt-stride` line; dropped this side's fsd line (renamed on main) |
| `Cargo.lock` (testbench deps) | the same | main's list, plus `redoubt-stride` in sorted order |
| `docs/kernel/scheduling.md` (Residual risks) | main's old 10 ms decision-wake text plus its new "Fair kernel entry" (R78) bullet, against this branch's 1 ms decision-wake text | kept this branch's text, followed by main's R78 bullet |

`docs/testbench.md`, `docs/SECURITY.md` and `tests/size-budget.toml` merged without conflict. No case
of this branch names fsd.

## Gate

Logs are in this directory; consoles in `consoles/`.

| Gate | Result |
| --- | --- |
| host tests: loader, paging, ipclist, layout, signing, testbench (118), stride | all ok |
| docs, formatting, no-cruft, size-budget, unsafe-budget | PASS |
| build-rv64, build-rv32 | rc 0 |
| sched-debt-lift, -large-weight, -server-busy, -carve-inflation, -ties (rv64, rv32) | PASS |
| sched-cluster, sched-cluster-old-control (rv64, rv32) | PASS |
| sched-latency seed 1 (`--sweep 1..1`, rv64, rv32) | PASS |
| userland-boot, ipc-outcomes, bench-net-peer and its siblings (rv64, rv32) | PASS |
| **init-boot (rv64, rv32)** | **FAIL: "beamlet: no heap record found"** |

## init-boot

**How the check works.**
- The check is main's `42bc99847`: each server's heap peak is read from the stopped guest's RAM.
- `rt`'s `Heap::start` writes the record at the server's startup, so it exists only once beamlet has
  run.
- The guest stops when init's last expected line ("the boot is done") prints.
- init starts beamlet last, then continues straight to that line.
- So the witness needs beamlet to be scheduled before init's last line, and nothing in the case makes
  that happen.

**Evidence.**

| Build | Slice | init-boot |
| --- | --- | --- |
| release (the case's own) | 10 ms, IPC3's gate | PASS |
| release | 1 ms | FAIL on both widths |
| checked, scratch (rv64) | 10 ms (`slice-10ms`) | PASS |
| checked, scratch (rv64) | 1 ms | PASS |

- The scratch consoles are in `initboot-diag/`. Both scratch tomls were deleted; the worktree is clean.
- So the slice does not break init-boot in itself. The case depends on an ordering that timing
  decides: an ordering accident, like ties' was.

**Fix options** (not SCHED1's code; for the orchestrator or the memory package's owner):
- **(a)** The case waits for a line beamlet prints after its startup, so the stop comes after the
  record exists.
- **(b)** init waits for each server's first report before "the boot is done".
- **(c)** The memory witness reports "not started" for a server that never ran, instead of failing.
  This is weaker.
