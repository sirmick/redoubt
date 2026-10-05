# SCHED1 stable evidence inventory — 2026-10-04

The five surviving `target/testbench/SCHED1-*.md` reports were copied here before the next focused `cargo testbench` run and verified byte-for-byte with `cmp`. They preserve source hashes, commands, prior test results and excerpts. The root `.wash/local/SCHED1-report.md` and `.wash/local/SCHED1-relative-wait-ruling.md` remain in their original stable locations.

Both old raw cluster logs are unavailable under `target/testbench`: the prediagnostic seed3 rv64 run `run-1-1791155555700002952/sched-cluster-rv64-smp1.log` (recorded SHA256 `ec5bf8ebaa639a328d19c762d4e6d584e455264fb400da42ba9723c8669a86e4`) and the diagnostic seed3 rv64 run `run-1-1791157775978771536/sched-cluster-rv64-smp1.log` (recorded SHA256 `9a1a3293d2e1a613088b529961733ed64e4361fdd720ef3305619bb97895fcfe`). Their recorded excerpts and hashes are historical notes, not preserved raw evidence. No QEMU rerun was made to recreate them.
