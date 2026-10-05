# MEM1 calibration stopped on ambiguous rv32 stack paint

Source: `wp-MEM1` HEAD `6535a9acbd6eea8fcdc7142d291da0d95ed7a656`, with only the existing four dirty calibration files: `image/manifest.json`, `servers/init/tests/manifest.rs`, `tests/userland-boot.toml`, `tests/userland-read-only.toml`. Their preserved binary diff is `MEM1-6535a9acb-provisional-calibration.patch`, SHA-256 `07090083475360102f81ed2e3911ca64a69d07f984d13d2e4151441ae1eb229e`. The beamlet declaration of 16 pages is provisional and is not an accepted final value. The pre-calibration whole-bench failure evidence is separately indexed in `MEM1-evidence-index.md`.

All calibration commands used `cargo testbench <case> --arch <width>` inside the existing `redoubt-dev` Docker image, offline, UID:GID 1447391350:1447391350, main checkout mounted at `/work`, the MEM1 worktree as cwd, and firmware `/work/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper` and `/work/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper`. Each run archive contains the command, exit, source identity, stdout, guest console scan log, `image/manifest.json`, relevant case TOML and dirty diff. The rv32 failure archive also contains the raw QMP RAM dump.

| Case | Width | Exit | Run directory | Result | Archive SHA-256 |
| --- | --- | ---: | --- | --- | --- |
| init-boot | rv64 | 0 | `run-1-1791161965603502393` | PASS, ten scanned stacks | `f1bbd49c8a4ab4ce26318db555baf709901287bf0181bbc83a4460bafce8a196` |
| init-boot | rv32 | 0 | `run-1-1791162049175513163` | PASS, ten scanned stacks | `3a7500c6bb19d79a407b27bf09d896d0551a6c2de6f156a65969f6597d0ad437` |
| userland-boot | rv64 | 1 | `run-1-1791162121221284922` | Full shell and attack verdicts, then twice-peak failure: `fsd:system` 12,072 bytes requires 6 pages, declared 3; all ten peaks valid calibration evidence | `bd4b33ed5bdedb9bb51de82f007ced5c6b5a16c499878ac0eba344056e2b1c23` |
| userland-boot | rv32 | 1 | `run-1-1791162320073185698` | Full shell and attack verdicts, then scanner error: `beamlet: stack paint index 21332 is outside its stack`; no trustworthy peaks from this scan | `4e2edd2002f5c91b53ef4db54f0443476f217027d487b6b54ed617c5ff8ac069` |

The rv32 run used a 16-page beamlet declaration, whose paint table has 8,192 eight-byte units. Index 21,332 is outside it. The scanner's error does not prove how the word arose. Under the `MEM1-runtime-stack` architect ruling, ambiguous paint ends calibration; no guard was relaxed and no source was changed. `userland-read-only` rv64/rv32 were not run. QEMU was released after this stop. No six-run maxima, final page declarations, image bound, or acceptance can be inferred.

Partial measured peaks from the three valid scans, in bytes (max of init-boot rv64/rv32 and userland-boot rv64 only):

| Server | init rv64 | init rv32 | userland rv64 | Partial max |
| --- | ---: | ---: | ---: | ---: |
| keyd | 5224 | 4464 | 5224 | 5224 |
| consoled | 8328 | 7064 | 8328 | 8328 |
| bootfsd | 6936 | 5768 | 6936 | 6936 |
| blkd | 4408 | 3744 | 4408 | 4408 |
| netd | 4264 | 3344 | 4264 | 4264 |
| ipd | 8008 | 6624 | 8008 | 8008 |
| fsd:data | 6888 | 5392 | 6888 | 6888 |
| blkd:system | 4408 | 3744 | 4408 | 4408 |
| fsd:system | 5896 | 4128 | 12072 | 12072 |
| beamlet | 4704 | 3624 | 31784 | 31784 |

The valid rv64 userland run shows the provisional 3-page `fsd:system` declaration lacks the mandated twice-peak margin. That result is not a passing case and is not a final declaration. The original full bench at clean HEAD remains 396 PASS/5 FAIL, and the separate OpenSSH reference failure is BENCHENV1's Podman prerequisite.
