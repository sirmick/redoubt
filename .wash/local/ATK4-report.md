# ATK4 report: boot.md's partly-tested lines

- Branch `wp-ATK4`, worktree `/home/mcloonan/redoubt/.worktrees/ATK4`.
- Base `985ec0047` (main). Head `d7cfe6225`, one commit. Not pushed.
- Kernel and loader unchanged.

## Gates (all exit 0)

- `make -k -j4 -f scripts/jobs.mk set CASES="loader-rejects-truncated-elf loader-rejects-segment-data-cut loader-rejects-wx-segment verified-boot-rejects-tamper verified-boot-rejects-bare-archive verified-boot-rejects-short-initrd boot-rejects-no-seed boot-rejects-short-seed boot-rejects-no-timebase boot-rejects-irq-past-plic reboot-verifies-bundle"` three times on the committed tree: 22 PASS each (both widths), 0 FAIL. Logs `.tmp/ATK4/gate-run{1,2,3}.log`. Run through `cargo testbench` (jobs.mk's path with no prebuilt index); prebuilt was not made, to spare the shared machine a whole-tree build for eleven ten-second cases.
- Fixture-sharing boot cases, both widths: `init-boot init-reboot rustsbi-boot rng device bundle-mapped verity-signed verity-bad-signature verity-flipped-tree verity-wrong-root verity-rollback verity-host-tests`: 23 PASS (`.tmp/ATK4/fixtures.log`).
- `rv64/host-tests` PASS (loader, paging, signing, testbench host tests, incl. the six new ones).
- `cargo test -p redoubt-doccheck --test docs`: 2 passed (the checker caught one citation form, fixed).
- `rv64/formatting` PASS (nightly rustfmt); `git diff --check` clean.
- The pre-existing warning `field harts is never read` (tools/testbench/src/idle.rs:72) is on main, not from this change.

## Lines closed, and how

1. **Firmware fallback** — two bench host tests, no second firmware image needed: `host:testbench::a_missing_firmware_image_fails_the_case_rather_than_falling_back` (`rustsbi_prototyper` split into a pure `firmware_image(named, env, arch)`: a missing override or default is `Err` naming path and variable, which the run reports as the case's failure, a SKIP only under `--allow-skip`; a present file answers its canonical path) and `host:testbench::every_boot_names_the_checked_firmware_and_a_tree_only_when_edited` (`Image::qemu()` always carries `-bios <that path>`; `-dtb` only when a case edited a tree).
2. **R16: image cut short inside segment data** — `loader-rejects-segment-data-cut` (both widths). New corruption `truncate-segment-data = 64`: drops the section table and cuts 64 bytes before the end of the last loadable segment's data; the headers parse and the loader's slice `p_offset..+p_filesz` fails its bounds check. Verdict: `^loader PANIC`, `range end index \d+ out of range for slice of length \d+`, KMAIN forbidden. (Why not plain `truncate`: the linker puts the section table after the data, so any cut reaching the data fails `minimal_parse` first, which is the existing case.)
3. **R16: W+X segment** — `loader-rejects-wx-segment` (both widths). New corruption `segment-flags = "rwx"` on the first loadable segment; `libs/paging` refuses: `refusing a writable and executable mapping`.
4. **R16: truncated-image case rv64 only** — `loader-rejects-truncated-elf` now both widths; the toml says it was rv64-only since written, for no recorded reason (it arrived in 4b074d8a2 like that).
5. **R17: short or missing seed, missing timebase, initrd under 64** — new `device_tree` knob: the bench dumps QEMU's tree for the case's own command line (`-machine dumpdtb=`), edits it in place (`tools/testbench/src/fdt.rs`: drop → FDT_NOP fill, shrink → length rewritten and the tail NOP'd, set → cells overwritten; no byte moves), boots with `-dtb`. QEMU 10.2 does not reinsert a dropped `rng-seed` (checked by hand before the code: `.tmp/ATK4/noseed.dtb` boot → loader PANIC, exit 255). Cases, both widths: `boot-rejects-no-seed` (drop), `boot-rejects-short-seed` (shrink to 8) — loader's `the device tree has no usable /chosen/rng-seed`; `boot-rejects-no-timebase` (`timebase-frequency` set to 0: RustSBI prints nothing at all without the property, so it stays as 0, which the loader reads as none and writes no `Time`) — kernel's `PANIC in PID 1 … the loader reported no timebase (\`Time\`)`; `verified-boot-rejects-short-initrd` (`short_initrd = 63`, the signed bundle's first 63 bytes) — `initrd is too small to be signed`. All with `poweroff_status = 255`, KMAIN forbidden.
6. **R15/R17: signature cases rv64 only** — `verified-boot-rejects-tamper`, `verified-boot-rejects-bare-archive` both widths, with the same comment.
7. **Reboot through system_reset** — `reboot-verifies-bundle` (both widths): init-reboot's machine (manifest `tests/data/init/reboot.json`, quitter restarted five times, init's sixth exit reboots), expecting in order `loader: Redoubt … boot hart 0`, `bundle signature ok (N bytes)`, `KMAIN (clean boot)`, `init: rebooting: …`, then the same three again and `init: started quitter`. Verdict from the loader's and kernel's bare lines (consoled prefixes every server's) and init's.
8. **Argument block** (partly) — the one malformed `Devs` entry a device tree can produce: `boot-rejects-irq-past-plic` (both widths), a virtio slot's `interrupts` set to 1024; the loader passes it on, the kernel: `Devs: an interrupt beyond the PLIC's sources`.

## Lines left, and why

- Argument block: a tag past the end, a second `MREx`, a `Devs` entry naming RAM or a controller, wrapping or naming interrupt 0, a `Grnt` tag. Only the loader writes the block; the loader filters RAM-resident nodes and interrupt 0 itself (`loader/src/dt.rs`), controllers are `kernel_only` and never `Devs`, and no tree makes it emit `Grnt`, a second `MREx` or a bad length. The kernel has no host tests by design (`kernel/Cargo.toml`). Testing these needs a loader change (a test-only hostile-block knob): said and left. boot.md's argument-block status and R17 residual bullet narrowed to exactly this; the todo line narrowed likewise.
- R17: the kernel's own refusal of a missing `Seed` tag (`rand.rs`) stands behind the loader's refusal (`main.rs:203`) and is unreachable by a boot; the page's R17 status says so.
- DMA device past the sixteenth: ATK3's, with devices.md's line (todo line annotated, not struck).

## Negatives (each shown once, on rv64, then the cases restored from the commit; logs `.tmp/ATK4/negative-<case>.log`)

| case | mutation | result |
| --- | --- | --- |
| loader-rejects-truncated-elf | expect `invalid ELF: SomethingElse` | FAIL, guest exited while waiting |
| loader-rejects-segment-data-cut | plain `rng-test` | FAIL, forbidden `entering kernel` |
| loader-rejects-wx-segment | `segment-flags = "rx"` | FAIL, forbidden `entering kernel` |
| verified-boot-rejects-tamper / -bare-archive | expect `signature is fine` | FAIL, guest exited while waiting |
| verified-boot-rejects-short-initrd | knob removed | FAIL, forbidden output |
| boot-rejects-no-seed | `device_tree = []` | FAIL, forbidden `rng-seed: \d+ bytes` |
| boot-rejects-short-seed | shrink to 16 (the boundary) | FAIL, forbidden `rng-seed: 16 bytes` — 16 is accepted, as the page says |
| boot-rejects-no-timebase | words = [10000000] | FAIL, forbidden `KMAIN` |
| boot-rejects-irq-past-plic | words = [1023] (the boundary) | FAIL, forbidden `KMAIN` — 1023 is accepted |
| reboot-verifies-bundle | second `bundle signature ok` → `skipped` | FAIL, timed out waiting for it (90 s) |
| firmware host test | `firmware_image` exercised with a missing path and a wrong arch default | asserts the `Err` text; a fallback to any other path would fail it |

## Docs changed

- `docs/kernel/boot.md`: Firmware → built·tested (+2 host tests); Argument block → partly tested narrowed, +`boot-rejects-irq-past-plic`, +`boot-rejects-no-timebase` (6); Verified boot → tested (8); Randomness → tested (+2); R15 → tested (+short-initrd); R16 → tested (+2); R17 → partly tested narrowed to the unreachable `Seed` refusal and the host-tested refusals (15), the "two boot cases" sentence rewritten; Failure and restart → tested (5); the residual bullet rewritten.
- `docs/todo/kernel-attack-gaps.md`: boot section only — five lines struck with case names, the argument-block line narrowed, the DMA line marked ATK3's.
- `docs/testbench.md`: boot-knob list; Hostile inputs status (17) and text for `segment-flags`, `truncate-segment-data`, `short_initrd`, `device_tree`.
- `docs/SECURITY.md`: R15 and R16 rows → built with the new cases; R17 row's tests and residual.
- Summaries checked, no change needed: `README.md`, `GETTING-STARTED.md`, `docs/README.md`, `docs/kernel/README.md`, `docs/plan/m2-usable-shell.md` (its only reference is to the todo page as a whole), `tools/testbench/README.md` (does not list the knobs).

## Notes for the reviewer

- `Image.dtb` is `None` on the interactive and `launch --system` paths; only a case with `device_tree` dumps and edits.
- The dump runs the case's full QEMU line once more with `dumpdtb`; QEMU exits before the firmware runs. The edited tree sits beside the console log (`<case>-<w>-smp<n>.dtb`).
- `prepare` gained a parameter (`short_initrd`) rather than a struct; it already carries `#[allow(clippy::too_many_arguments)]`.

## Added after the first report (orchestrator's request)

- **Prebuilt gate**, on d7cfe6225: `make -f scripts/jobs.mk prebuilt` exit 0 — rv64: 307 cases, 0 failed, 148.4 s; rv32: 296 cases, 0 failed, 145.7 s (`.tmp/ATK4/prebuilt.log`). `target/prebuilt/{rv64,rv32}` now exist in this worktree.
- **Rebase check against train 34's head dfa6d8875** (`git merge-tree --write-tree dfa6d8875 wp-ATK4`, no rebase done): exit 0, clean, merged tree `2873ff75c`. Files both sides changed: `docs/testbench.md`, `docs/todo/kernel-attack-gaps.md`, `tools/testbench/src/{case,main,qemu}.rs` — all in disjoint hunks. The merged todo page keeps ATK2's and ATK3's sections and my boot section (10 struck lines in all). Their testbench hunks (QMP socket directory, scheduling/device knobs) touch neither `Image`, `prepare`, `rustsbi_prototyper` nor `Corruption`; in the merged tree every `Image { .. }` site carries `dtb`. Not compiled as a merged tree (no scratch worktree made); nothing seen that would fail to.
