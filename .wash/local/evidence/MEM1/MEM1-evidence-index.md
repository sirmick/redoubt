# MEM1 test evidence preserved before calibration

Source identity: `wp-MEM1` at `6535a9acbd6eea8fcdc7142d291da0d95ed7a656`. The whole bench ran on that clean head, before the provisional calibration diff. Its unfiltered `cargo testbench` exited 1: 396 PASS, 5 FAIL. `MEM1-whole-bench.log` is the original stdout. `MEM1-6535a9acb-whole-run-artifacts.tar.gz` preserves 388 files from `target/testbench/run-1-1791157885812581464`: all 368 `.log` files, packet captures, and the two generated manifests. The four failing userland console logs and the two passing `init-boot` memory-scan logs are in the archive. The fifth failure is `bench-ssh-loopback-openssh`, which lacked Podman and has no guest log.

`MEM1-6535a9acb-provisional-calibration.patch` records the current four dirty paths after that bench: the historical 16-page beamlet declaration, its provisional bound assertion, and `memory = true` on both userland cases. This patch is calibration preparation, not accepted final source.

Previously reported focused `init-boot` raw directories `run-1-1791155890542911418` (rv64) and `run-1-1791155918896927773` (rv32), plus other earlier focused refusal logs, are no longer present under `target/testbench`; testbench pruned them. The whole-bench archive contains later `init-boot`, `init-refuses-stack`, and `init-refuses-bound` logs on both widths from the same clean head. No old run was repeated to reconstruct lost raw artifacts.

SHA-256:
- `MEM1-whole-bench.log`: `b507f8ba567ee17a6b154602c2d6f7ebba11604efe6695ba1cfac7d1cd050da1`
- `MEM1-6535a9acb-whole-run-artifacts.tar.gz`: `b790bfe6553b627aeb7981226f01061bd61fddcbc4959423acec41ac5cc18cda`

For each future calibration or final `cargo testbench` call, copy its stdout, console scan log, and any trace into this directory before the next invocation. Record source commit plus dirty-diff identity, command, exit code, and run directory. A failed twice-peak margin can inform final sizing; a guest fault, ambiguous/missing paint, or cap breach stops calibration under the architect's ruling.
