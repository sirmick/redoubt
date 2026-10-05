# MEM1: measure stacks through the required runtime workloads

Accepted measurement correction under existing MEM1 scope, tracked on
`MEM1-runtime-stack`. This is a measurement ruling, not package acceptance; the QA
remains blocking until corrected measurements and final acceptance evidence exist.

## Diagnosis

Whole-bench evidence at clean `6535a9acb` is in
`.worktrees/MEM1/target/MEM1-whole-bench.log` and the four `userland-boot` /
`userland-read-only` logs under
`.worktrees/MEM1/target/testbench/run-1-1791157885812581464/`.
Both cases fault before the shell banner, after beamlet reports reading `system.index`.

| Width | Faulting store address | PC | SP |
| --- | --- | --- | --- |
| rv64 | `0x7fffb648` | `0x00271df4` | `0x7fffb630` |
| rv32 | `0x7fffc500` | `0x0030d736` | `0x7fffc4f0` |

The three-page initial stack maps `[0x7fffd000, 0x80000000)`. Both store addresses and
stack pointers are below it; fault code 15 is `StorePageFault`. The exercised initial
thread cannot fit in that allocation. This is distinct from beamlet's four-page
console-reader stack and the loader-started init's 32-page reservation.

`init-boot` stops after init's final boot line and its 50 ms grace period, then scans.
Its logs have no beamlet index or shell banner yet. The reported beamlet peaks of
4704 bytes on rv64 and 3624 on rv32 measure early startup, not the required VM/shell
workloads. Twice those peaks does not cover the later path. Other beamlet cases use
separate manifests and do not establish adequacy of this image's three-page stack.

The release ELFs are stripped; address-to-source lookup returned unknown. The precise
faulting function, frame and eventual runtime peak remain unproved. Source enters
`beamlet_redoubt::run` after the index report, then constructs, spawns and runs the VM.
Do not derive a final declaration from the fault address alone. The separate missing
`podman` failure for the OpenSSH case is unrelated.

## Accepted correction

- Retain `init-boot` as early-boot evidence. Measure `userland-boot` and
  `userland-read-only` with `memory = true` after their existing complete verdicts,
  on both rv32 and rv64. Reaching the banner alone is insufficient: preserve their
  module refusal, successful call and read-only attack checks.
- Restore beamlet's historical 16 pages for calibration only, retaining paint and
  the strict twice-peak scanner. This is not an accepted final allocation or an
  allowance for speculative headroom.
- For each image server, take the largest measured peak in bytes across all three
  required cases on both widths, and set
  `stack_pages = ceil((2 * max_peak_bytes) / 4096)`.
- Rerun every required scan with the final declarations. Recompute the standard
  image's root bound and any doubling claim from those declarations; keep special
  case bounds distinct. Update owning pages and the measurement table accordingly.
- Preserve full fault, missing-paint, duplicate-paint and no-restart guards. The
  read-only case's additional client parks after its verdict and remains subject
  to the merged-manifest scan; do not silently exclude it.

A valid peak from a calibration run that fails the twice-peak margin can inform the
next declaration, but the failed run is not acceptance. If calibration still faults,
paint is ambiguous, or the computed declaration exceeds the existing 128-page cap,
stop and return evidence. Do not raise the ceiling, weaken the scanner, omit a case,
or add unexplained headroom. A wider cap or runtime redesign requires a separate
evidence-backed ruling. No present human tradeoff is needed to correct workload
coverage under the already accepted twice-peak rule.

The paint measures paths exercised by these cases; it is not a bound on every future
program or an adversarial proof. It measures the declared first-thread stacks, not
all separately allocated thread stacks. Report those limits without using them to
excuse a failing required workload.

## Ownership and acceptance

The orchestrator coordinates implementation and QEMU release. Concurrent dirty
preparation of beamlet's calibration allocation and the two memory cases was observed
during diagnosis and was not treated as tested `6535a9acb` content. The Architect
performed no source/test edits or test runs. Final declaration remeasurement, bounds,
documentation, full required gates and exact-content review remain necessary; only
the authorized acceptance process resolves the blocking QA.
