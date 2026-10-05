# BEAM7 acceptance evidence source

- Package branch: `wp-BEAM7`
- Base: `9212f6f600591abb3ea78ef450e0d6f4d517ab92`
- Tested head: `6a79d0e1356493f5307b2af19934b4f3d8515c79`
- Unfiltered whole bench: `cargo testbench` in the existing `redoubt-dev`
  container, UID:GID `1447391350:1447391350`, `--network none`, root at
  `/work`, workdir `/work/.worktrees/BEAM7`, project-local Cargo/Rustup caches,
  and both prebuilt RustSBI image variables. Exit 1: 399 PASS verdicts; one
  environment failure in `bench-ssh-loopback-openssh` because `podman` is absent.
  No skip, case exclusion, install, or runner modification.
- Whole-bench stdout source: `target/beam7-whole-tier-a.log` in the worktree.
- Whole-run artifact source:
  `target/testbench/run-1-1791159626245169437/` in the worktree.
- Stable copy: `whole-run/` holds every regular `*.log`, `*.pcap`, `*.json`,
  and `*.trace` file from that run, preserving relative paths (399 files,
  71,289,238 bytes). The log set includes guest console and scheduler trace
  records. Generated images, bundles and compiled objects are not copied.
- Stable `gate-logs/` holds all 26 worktree `target/beam7-*.log` files,
  including whole-suite stdout, final docs/formatting/size/unsafe, rv32 builds,
  focused machine/host results and the deliberate negative control.
- `review-evidence.md` is the worktree review brief copied at retention time.
- `SHA256SUMS` hashes the copied whole-run artifacts, gate logs, and brief.

Older focused and negative-control `target/testbench/run-*` directories remain,
but none contains raw `*.log` files; the testbench had pruned those raw logs
before this retention instruction. Their saved `target/beam7-*.log` stdout
files remain available, and the unfiltered current run supplies fresh raw
guest logs for every BEAM7 machine case. The negative-control raw guest log
is unavailable; its saved stdout records the expected assertion failure.
