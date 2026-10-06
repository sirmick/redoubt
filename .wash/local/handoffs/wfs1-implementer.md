# WFS1 handoff (wfs1-implementer), second checkpoint

## Branch state
- Worktree /home/mcloonan/redoubt/.worktrees/WFS1, branch wp-WFS1, head faa51ede2, clean, base main bac001578. Six folded logical commits:
  1. f0df67c72 walfsd: the page specifies walfs (docs/servers/walfsd.md, SUMMARY, servers/README naming/table/both graphs, littlefsd Purpose sentence)
  2. 3178fd3fb walfs: crate + tests/model.rs + tests/common/{mod,ops}.rs + root Cargo.toml member/exclude + tests/walfs-host-tests.toml + size-budget (max_lines 1705) / unsafe-budget (0) rows; message has "Size budget: libs/walfs: ..." and "Unsafe budget: walfs: ..." lines
  3. baea98461 tests/crash.rs
  4. 34be91e99 tests/hostile.rs + tests/common/exercise.rs
  5. dd67c35fa libs/walfs/fuzz (+Cargo.lock, rust-toolchain nightly-2026-05-11), tests/formatting.toml root, page "The format" built (31 tests listed), testbench.md Hostile inputs paragraph
  6. faa51ede2 testbench fs = "walfs" (tools/testbench/src/disk.rs, tools/testbench/Cargo.toml, Cargo.lock), testbench.md Disks section, page "The packer" built, docs/plan/m1-separation.md progress bullet + "Not built: walfsd"
- To change a commit: `git commit --fixup=<sha>`, then `GIT_SEQUENCE_EDITOR=true git rebase -q -i --autosquash bac001578` (works non-interactively).

## Traps / must fix first (the docs gate FAILED, rc=1)
docs checker findings (target/jobs/docs.log):
- `docs/servers/walfsd.md:1: C9`: the `##` headings must be exactly Purpose, Interface, Authority, Security properties, Failure and restart, Residual risks, Why. **Add `## Security properties`** (planned; e.g. one sentence that walfsd will keep littlefsd's R47/R49/R50 as restated by the server's package, plus one **Open:** line if planned; no R-rule rows or SECURITY rows, per the brief).
- `walfsd.md:325` and `:332: C1`: each planned section needs exactly one `**Open:**` line → Authority and Failure and restart each need one **Open:** line (e.g. Authority: "**Open:** none."? check what C1 accepts; resolver.md uses "**Open:** none.").
- `libs/walfs/src/lib.rs:6: C11`: a commit hash is refused in a comment. Name the xv6 reference without the hash (the brief asked to "name the commit": name it by date or tag, e.g. "mit-pdos/xv6-riscv as of 2026-10 (kernel/fs.h, kernel/log.c)", and put the full hash in the report instead).
These edits go into commit 1 (page) / commit 2 (lib.rs), or commit 5 if Security properties belongs with the status changes. Rerun `docs`.
- Other traps: the hash region is option (b) as the orchestrator decided: 127 slots + own hash at 4064; no log slots (slot i = block i for i = 0, else block i+33); hash block's own slot is zero. All implemented and on the page.
- The commands now go through q: `/home/mcloonan/redoubt/scripts/q run --cores 4 -- cargo test -p walfs --release`; gates via `make -k -f /home/mcloonan/redoubt/scripts/jobs.mk -C <worktree> rv64/<case>`. Read the new resume note first (B18/B19 change how cases run).
- cargo-fuzz installed by me in ~/.cargo/bin. libs/walfs/fuzz/target is untracked: never add it.

## Results on the final head faa51ede2
- Fuzz: image 300 s, 72,475,772 runs, no findings (cov 37: random bytes never pass the superblock's hash, by design); mutate 300 s, 111,369 runs, cov 1196, ft 3562, corpus 633, no findings.
- walfs builds for riscv64gc (exit 0) and riscv32imac (exit 0); build-rv64 rc=0; build-rv32 rc=0.
- PASS: size-budget, unsafe-budget, no-cruft, formatting; smoke set: init-boot rv64/rv32, userland-boot rv64/rv32, ipc-outcomes rv64/rv32.
- FAIL: docs rc=1 (above).
- KILLED by the checkpoint shutdown, NOT verdicts, rerun: rv64/walfs-host-tests (rc=241), rv64/host-tests (rc=143), rv64/bench-net-peer and rv32/bench-net-peer (rc=143).
- Earlier, direct `cargo test -p walfs --release` on the final code: all pass (1 lib + 10 model + 6 crash + 11 hostile). Hand mutations (reverted) failed the crash tests (header before log blocks; no sync before the clear) and the flipped-bit test (no hash check).

## Next
1. Fix the three docs findings, fold, rerun docs.
2. Rerun walfs-host-tests, host-tests, bench-net-peer on both widths (and docs).
3. Write /home/mcloonan/redoubt/.wash/local/WFS1-report.md: paths and commits; gates with exit codes; fuzz lengths and findings; lines 1705 / unsafe 0; the page lines quoted exactly; summaries checked (README.md, GETTING-STARTED.md: no claim affected; m1-separation.md, servers/README.md, littlefsd.md, testbench.md: updated; SECURITY.md: no rows, nothing serves the format); xv6 commit 06aad25c735fd3159bdfae5680be4eab7b1668b2; paths beyond the brief's ownership (tools/testbench/Cargo.toml, tests/formatting.toml, docs/plan/m1-separation.md, testbench.md Disks); design notes (no per-handle buffer, short write when full, orphan list, sha2 0.11 since libs/sha256 is not on main). Then message_send `result` (head, commits, every gate's exit code) and member_update assignment_results complete for db0e8767b6d0b3a165b12e37759bda9e.
