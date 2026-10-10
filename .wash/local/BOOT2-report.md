# BOOT2 report: the session image streamed in 16-page lends; the boot profile ends at the first prompt drawn

Branch wp-BOOT2, worktree /home/mcloonan/redoubt/.worktrees/BOOT2, head a3e4e6fd3, ONE commit (16
files) on main 311b0b22b (K26, B30, B32 and the rest in; rebased from d0a19ac6c without conflict). Tier A (init, the steward, a VM native). Design
checkpoint and ruling: .wash/local/BOOT2-design.md (A only; B declined; C ruled out).

## What was built

- **init** pushes a public entry in `add` calls of 32 KiB through a 16-page lend (bound.rs
  LEND_PAGES 2 -> 16 = MAX_LEND_PAGES; init.rs CHUNK = 8 pages): 128 calls for beamlet's 4 MB,
  not 1,024. The chunk is a power of two on purpose: bootfsd's entry `Vec` doubles from the first
  chunk, and 60 KiB chunks gave a 3.75 -> 7.5 MiB series whose last doubling (old + new
  capacity live) put rv32's 3.76 MiB image at a 2,884-page peak against bootfsd's 3,080 cap
  (rv32 userland-boot failed); 32 KiB lands on 4 MiB and the peak is 1,540, as on main. init's bound grows by the lend's 14 pages: the image's is
  564 (budgets.md quotes it), beamlet-boot's 460 (its first expect pins it); bound.rs's tests
  sum the lend term from the constant, manifest.rs's one by the figure with its comment.
- **the steward** streams a session's image through a 16-page lend (LEND_PAGES = MAX_LEND_PAGES):
  a 9P read carries the lend's iounit, so a 64-page launch batch is 4 reads, not ~8.
- **`beamlet:prompt_drawn/0`**, a `Platform` hook (default nothing) the shell's driver calls once
  after it has drawn its first prompt on the console (not with `:messages` input, the tests');
  Redoubt's platform, in a boot-stats build, writes `beamlet: first prompt drawn [t=N]`, the end
  of the span the boot-time target is on. The old `first console read` line stays (it is the
  driver taking the console, before the banner) and the `boot-stats: loads` line with it.
- **The cases.** boot-profile and -unverified expect: first console read, loads, banner, first
  prompt drawn (the target's bound, unanchored: it is drawn beside the prompt on its line), B30's
  echoed input line, 55, ...; the input is sent at the prompt stamp. Their comments carry the
  measured figures. beamlet-boot's bound expect 446 -> 460.
- **Pages.** beamlet.md: the table gains a row (to the first prompt drawn, 16-page lends), the
  breakdown paragraph has the four spans both widths, verified and unverified, and what each
  line means; the native in the platform tables; testbench.md's boot-stats sentence names both
  lines; steward.md's session bullet says the stream and its lend; budgets.md's bound 550 -> 564.

## Measured (boot-profile, icount shift=3, seed 1, guest time)

| | rv64 verified | rv64 unverified | rv32 verified | rv32 unverified |
| --- | --- | --- | --- | --- |
| servers up (sshd started) | 0.58 | 0.53 | 0.72 | 0.66 |
| steward started (the push before it) | 1.86 (was 3.15) | 1.80 | 2.26 (was 3.96) | 2.17 |
| session VM's boot pack read | 5.76 (was 7.66) | 3.64 | 6.62 (was 9.13) | 4.43 |
| first console read (the driver's) | 11.83 (was 13.68) | 9.54 | 13.22 (was 15.68) | 10.86 |
| first prompt drawn (the target's line) | 14.67 | 12.19 | 16.25 | 13.70 |

(On main 311b0b22b with B32's 11,904-page session; 32 KiB adds. The first round, on d0a19ac6c with
60 KiB adds, read 1.74 / 5.61 / 11.64 / 15.13 s on rv64 verified.)

The push fell from 2.6 to 1.2 s (rv64) and 3.3 to 1.4 s (rv32); the stream by about 0.6-0.9 s
(the pack's read, ~2.8 s verified / ~1.2 s unverified, is the rest of that span). The shell's
start under its driver, first read -> first prompt, is 3.5-3.7 s on both widths and in no span
before: the old rows (15.4 / 13.2 / 17.6 / 15.3 s) were taken when the first read WAS the prompt,
before SHELL2; the prompt pre-A, post-SHELL2 would have been ~17.2 s rv64 / ~19.4 s rv32. So the
20 s target's rule (slowest prompt plus a tenth, rounded up to 5 s: 16.7 x 1.1 = 18.4 -> 20 s)
gives 20 s still, not the node's hoped-for 15 s: SHELL2's driver costs what the lends saved.

Residual: a 15-page `add` still costs ~17-20 ms (1.2 s / 70) and a 16-page read about the same:
~40 insns a byte somewhere on the lend path or in bootfsd's `Vec` growth; the wire codec copies
by slice. Not chased (a measurement node if wanted).

## Gates (q / jobs.mk; scratch /home/mcloonan/redoubt/.tmp/BOOT2/)

On the pre-fold tree (code byte-identical to fe95ce9e8; only pages and two case files moved):
PASS both widths steward-restart, steward-restart-reboot, init-restart, beamlet-files,
beamlet-console, aio-many-reads, userland-boot, init-boot, ipc-outcomes, bench-net-peer; PASS
size-budget, formatting, docs; cargo test -p redoubt-init 41/41 and -p redoubt-steward-server
59/59; cargo test -p beamlet-vm 74/74; ./test-shell every stage.
On fe95ce9e8 (d0a19ac6c base, prebuilt rc 0): PASS beamlet-boot rv64 and rv32 (bound 460), init-boot
rv64 and rv32 (the image's bound 564), docs, boot-profile and -unverified rv64 and rv32.
After the rebase onto 311b0b22b (4e359b9c8, 60 KiB adds): PASS both widths boot-profile,
boot-profile-unverified, beamlet-footprint (B32's fix), beamlet-boot, steward-restart, userland-boot
rv64; docs, size-budget, ./test-shell; FAIL rv32 userland-boot (bootfsd's 2,884-page peak, mine:
above). On c0fb7ba76 (32 KiB adds): PASS userland-boot rv64 and rv32 (bootfsd 1,540 of 3,080, as
main's), docs, boot-profile and -unverified rv64 and rv32. a3e4e6fd3 moves figures on pages and in
two cases' comments only: docs PASS on it.

Not mine: **beamlet-footprint fails on main d0a19ac6c itself** (and so here): "heap needs 11804
pages for twice its 5902-page peak, capped at 10989" on rv64 (11408/5704 on rv32): the shell's
peak under SHELL2's driver crossed half the cap 6318394c6 set. A node for the orchestrator.

## Documentation check

beamlet.md (breakdown, table, platform tables), testbench.md (boot-stats lines), steward.md
(sessions), budgets.md (the bound), init.md ("pushes the public entries": no chunk size stated,
unchanged), bootfsd.md ("add appends data": unchanged), native.md ("64 pages at a time": the
launch batch, unchanged), README.md and GETTING-STARTED.md (no boot-time claim), plan/m1 (no
figure). All checked.
