# B3 progress

## Checkpoint 1: deliverable 1 (commit 1bdef2760 on wp-b3)

- `tools/testbench/src/qemu.rs`: QEMU stderr piped, read on its own thread into a bounded tail
  (last 20 lines, 512 bytes each). `Console.seen` records whether any console line arrived.
  `exited()` builds the verdict: always appends `QEMU exited with <status>`; when no console line
  was seen or the status failed, appends the stderr tail (joined ` | `) and writes it to the log.
  Used for the expect-loop `Line::Exited` and the poweroff status mismatch. The reader is given
  1 s after QEMU exits (a peer helper may hold the pipe), never joined unbounded.
- `tests/bench-qemu-early-exit.toml` (new): `icount = "no-such-option"`; must_fail anchors on
  `QEMU exited with exit status: 1: .*-icount no-such-option: icount: Invalid shift value`.
- `tests/bench-cbo-self-unrefused.toml`, `tests/bench-attack-forgery.toml`: must_fail now ends
  `: QEMU exited with exit status: 0` (the message gained the status per the brief). These are
  existing bench-* files, outside "new bench-*.toml"; the change is forced by deliverable 1.

## Evidence

- `in-dev cargo testbench bench-qemu-early-exit` → PASS (exit 0).
- Deliberate break (stderr back to null, empty tail) → FAIL "... exit status: 1: nothing on
  stderr", exit 1. Restored.
- `in-dev cargo testbench bench-cbo-self-unrefused` / `bench-attack-forgery` → PASS.
- `in-dev cargo test -p testbench` → 50 passed, exit 0.
- `in-dev cargo +nightly fmt --all --check` → exit 0.
- `in-dev cargo testbench bench-`: all pass except the 7 `bench-ssh-loopback*` cases, failing
  `bench error: No such file or directory (os error 2)`; the container has `ssh` but no `sshd`,
  which looks environmental (not touched by this change). Not yet confirmed against main.

## Next

Deliverable 2 (preflight in main.rs, both widths, --allow-skip → SKIP) with host test;
deliverable 4 pages; whole bench; doccheck.

## Checkpoint 2: deliverables 2, 2b, 4; branch folded

wp-b3 = cc6b697f2 + three commits, each with its pages, each `cargo test -p testbench`,
doccheck and `fmt --all --check` green on its own:
- 4f86defd6 testbench: a guest that dies at once is reported with QEMU's own error
  (qemu.rs stderr; bench-qemu-early-exit; the two forced must_fail edits; testbench.md early-exit
  paragraph and statuses) — 50 tests.
- 9841e9f39 testbench: the bench checks its QEMU takes its options before a boot
  (qemu::usable/probe, main.rs wiring, host test a_qemu_lacking_an_option_is_named; page; QEMU
  10.1 in GETTING-STARTED) — 51 tests.
- 026683904 testbench: an ssh too old for Redoubt's loopback server is named
  (ssh::redoubt_usable via `ssh -G`, path context on server-log read and session files, host test
  an_ssh_lacking_an_option_is_named; WarnWeakCrypto bullet; OpenSSH 10.1 in GETTING-STARTED) — 52.

Breaks shown: QEMU probe forced Ok → host test fails; ssh probe forced Ok → host test fails;
simulated bad QEMU option → boot case FAIL, SKIP under --allow-skip; container OpenSSH 10.0p2 →
bench-ssh-loopback-exit FAIL with the version message, SKIP under --allow-skip.
unsafe: none added. The earlier whole-bench run was invalidated by the fold's checkouts;
the whole bench reruns after the image has OpenSSH 10.1.

## Final: whole bench on the rebuilt image (OpenSSH 10.3p1, QEMU 11.0.2)

`in-dev cargo testbench --allow-skip` at 026683904: exit 1. 274 PASS, 1 SKIP
(bench-ssh-loopback-openssh: "the reference sshd's container: podman is not installed"),
1 FAIL: vendor-check. Its tests beamlet_builds_no_unpinned_registry_crate and
the_patches_point_at_vendor fail with "cargo metadata failed: no matching package named `pcre2`
found / location searched: crates.io index / required by beamlet-re (userland/otp/re) / offline
mode". That is cargo's offline registry in the rebuilt image lacking pcre2; B3 touches nothing
there (tools/testbench, tests/bench-*, two pages). Rerun alone: same failure.
(The log /tmp/b3-bench.log also got lines from the stale earlier run; counts are from the new run.)
