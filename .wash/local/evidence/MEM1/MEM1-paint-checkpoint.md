# MEM1 paint-scanner source and host checkpoint

Branch `wp-MEM1`, corrected scanner HEAD `a1187eac30e52595a896ebccaf0833d8aa6d4f04` (parent `6535a9acbd6eea8fcdc7142d291da0d95ed7a656`). Commit `testbench: qualify stack paint before checking its index` changes only `tools/testbench/src/memory.rs`; the file was read in full before commit. Exact review diff: `/home/mcloonan/redoubt/.wash/local/evidence/MEM1/MEM1-paint-scanner-final.patch`, SHA-256 `dd35ddea05785ac908d47fa6665c77fa7aafc60a0b78de85be45beb0e57084ed`.

The scanner now applies its existing physical-page-slot equality before validating the decoded index. A non-congruent magic/tag/index candidate credits no untouched paint; the overwritten original slot stays missing. A congruent out-of-range candidate still fails. New tests reproduce the rv32 dump's overlapping partial-word bytes, assert the missing slot increases the peak, and prove a page-congruent out-of-range control fails. Existing tests retain qualified duplicate, missing-paint, insufficient twice-peak margin and valid-scan checks. No fault/restart guard or page cap changed.

Focused commands in existing offline `redoubt-dev` Docker UID:GID 1447391350:1447391350, with checkout caches mounted:

- `cargo testbench memory-host-tests`: exit 0 on final formatted tree (`target/MEM1-paint-memory-host-tests-final.log`).
- `cargo testbench formatting`: exit 0 on final formatted tree (`target/MEM1-paint-formatting-final.log`). An earlier attempt exited 1 only for scanner formatting; that log is also preserved.
- `cargo testbench docs`: exit 0 at corrected HEAD (`target/MEM1-paint-docs.log`).
- `git diff --cached --check`: exit 0 before commit; `git diff --check`: exit 0 after.

Stable copies of the logs and diff are in `/home/mcloonan/redoubt/.wash/local/evidence/MEM1/`. The original four calibration files remain dirty and unchanged at the same binary diff SHA-256 `07090083475360102f81ed2e3911ca64a69d07f984d13d2e4151441ae1eb229e`. They are not in the scanner commit. The archived raw rv32 dump remains untouched. No QEMU run occurred in this correction.

Affected documentation and summary check: `docs/testbench.md` (the memory-budget section already says the scanner uses encoded physical-page offsets and counts the lowest missing unit), `README.md`, `GETTING-STARTED.md`, and `docs/README.md` (no scanner-specific status claim), and `docs/plan/m1-separation.md` (no scanner status claim). None needs a correction for this ordering change. `tools/testbench/README.md` does not exist. The broader MEM1 stack-bound documentation remains pending final all-six measurements; no final pages or bound are inferred here.

Red review should inspect the exact patch and the preserved raw dump diagnosis in `/home/mcloonan/redoubt/.wash/local/MEM1-paint-diagnosis.md` before any renewed QEMU window. `MEM1-runtime-stack` remains blocking. Whole workloads, final declarations, bounds/docs and Tier A acceptance gates remain pending.
