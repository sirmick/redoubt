# MEM1 rebase report (mem1-implementer-5, 2026-10-05)

Git only; no cargo, no QEMU run (machine hold). Never pushed.

## State

- Before: wp-MEM1 `cd73175b5`, clean, base `e9f2fcb95`; no live qemu/cargo process of mine.
- `git rebase --signoff main` onto main `6a7d2385e`.
- After: head `4a080b1dc5fb98c4ba200a35b4bc415eee56a16d`, base `6a7d2385ede136714763252a0b81d0be2959f8ac`,
  nine commits, same messages plus `Signed-off-by: Michael <sirmick@gmail.com>`; tree clean.

| old | new | subject |
| --- | --- | --- |
| 2ba44be21 | df97da8fb | client: paint each launched first-thread stack for measurement |
| e903d845f | 41c51fe69 | testbench: account for measured stack code in size ceilings |
| 197b6e3c1 | 8293e1210 | init: preflight each server stack before launch |
| 71b253a88 | ed168588f | testbench: measure painted server stacks after boot |
| 24b0fc579 | dba3bea90 | docs: clarify client launch coverage and stack controls |
| fb63dfb3f | 8f64e9389 | docs: record measured server stack bounds |
| 6535a9acb | e9d0ff2e3 | docs: reconcile image and subsystem summaries |
| a1187eac3 | 2dcc7c8ca | testbench: qualify stack paint before checking its index |
| cd73175b5 | 4a080b1dc | image: size first-thread stacks from six boot workloads |

## Conflicts

One, in `ed168588f` (old 71b253a88), `tools/testbench/src/qemu.rs`, the `use` block: main
(BENCHENV1/TOOL1, QEMU parent-death via `pre_exec`) added `use std::os::unix::process::CommandExt;`
where MEM1 added `use std::os::unix::net::UnixStream;`. Resolution: keep both, in rustfmt order
(`net` then `process`). No other path conflicted.

## Range-diff

Every commit shows `!` only because of the sign-off trailer; commits 5–7 (no body) also gain the
blank line before the trailer. The only content difference is commit 4's qemu.rs context line
(`CommandExt` import from the base) — the forced change above. Check: the `+/-` lines of
`git diff e9f2fcb95 cd73175b5` and `git diff main HEAD` are identical (diff exit 0); both
32 files, +569/-64. No other forced change.

## What the base change could affect (by reading)

- **QEMU invocation (BENCHENV1/TOOL1, qemu.rs):** `-run-with exit-with-parent=on` replaced by
  PR_SET_PDEATHSIG in `pre_exec`; guest-visible options unchanged. MEM1's `-qmp unix:...`, QMP
  `stop` and `pmemsave` are old QMP commands, fine on the stated floor QEMU 8.2 and this host's
  10.2.1. No effect on the scan mechanics or verdict path expected.
- **BENCHENV1 guest (ssh_guest.rs, ssh.rs):** only the OpenSSH loopback case; not a MEM1 case,
  no shared code with memory.rs. No effect.
- **BEAM7 testbench workspaces (case.rs HostTests.workspace/features, build.rs):** host-test
  cases only; `memory-host-tests` uses the root workspace default. No effect expected.
- **Guest code that the scan measures — two real risks, need the granted runs:**
  1. BEAM7 changed beamlet (`userland/otp/vm/src/vm.rs`, `platform.rs`, `redoubt/src/lib.rs`:
     Found/Absent/Refused lookup). beamlet is a scanned image server.
  2. TOOL1 added `rust-toolchain.toml` pinning Rust 1.99.0; the scans ran in the old container on
     "stable via rustup", version not recorded in the evidence. A different compiler moves every
     server's frame sizes.
  The rule fails when `ceil(2*peak/4096) > declared`, so a peak may grow to `declared*2048`.
  Headroom over the archived peaks: ipd 184 B (8,008 of 8,192), fsd:system 216 B (12,072 of
  12,288), keyd 920, beamlet 984 (31,784 of 32,768), bootfsd 1,256, fsd:data 1,304, blkd and
  blkd:system 1,736, netd 1,880, consoled 1,912. ipd and fsd:system are the most exposed.
  If a memory case fails on the new base, the declarations (image/manifest.json) and the
  docs/testbench.md table need re-measuring, which changes reviewed content.
- Size budget: the toolchain pin could also move `size-budget` ceilings MEM1 raised (41c51fe69);
  checked by the granted gates.

## Next (on grant)

Whole bench both widths, `init-boot`/`userland-boot`/`userland-read-only` per width (memory
scans; compare peaks to the table), `memory-host-tests`, `init-host-tests`, `docs`,
`formatting`, `size-budget`, `unsafe-budget`, rv32 build.

# Fix round 1 and fold (mem1-implementer-5, 2026-10-05)

Source and git only (machine hold); no cargo, no QEMU. Never pushed.

## Base and head

- Base: main `6a7d2385ede136714763252a0b81d0be2959f8ac`.
- Final head: `ef40ecf3fe102c4958a662433e2271e675620185`, five commits, each with
  `Signed-off-by: Michael <sirmick@gmail.com>`; `git status` clean.
- Replaces the 9-commit `4a080b1dc`. `git diff 4a080b1dc ef40ecf3f`: 13 files, +75/-59, all of it
  the fixes below.

| commit | subject |
| --- | --- |
| f5365eef4 | client: paint each launched first-thread stack for measurement |
| 7ddee958a | init: preflight each server stack before launch |
| eeb255880 | testbench: measure painted server stacks after boot |
| a94f73fd4 | docs: describe declared server stacks and their measurement |
| ef40ecf3f | image: size first-thread stacks from six boot workloads |

How the old commits were folded:
- old 2 (size ceilings) was split. The stub and client raises moved into the client commit, the
  init raise into the init commit, so each raise sits in the commit that adds its code.
- old 8 (qualify paint) went into the testbench commit.
- old 5, 6 and 7 (docs) became one docs commit.
- old 9 (image) keeps the measured declarations and the userland memory cases.

The init commit no longer adds the early image declarations (beamlet at 3 pages, which the shell
outgrew). The image keeps the 16-page default until the image commit, so the image bound test and
budgets.md's 507 are already true at the init commit, and every intermediate commit should boot.
That has not been run yet.

Every message now has a component subject and a body giving the reason, wrapped at 72 columns.
The `Size budget:` lines are in the commits that raise the ceilings. Co-Authored-By trailers are
the original models' plus Claude Opus 5.5.

## Findings

- red 1: APPLIED. memory.rs module doc and testbench.md now say "any guest, including another
  server, can forge" the paint.
- red 2: APPLIED. The QMP socket is always removed: a `Removed` drop guard in qemu.rs `run`,
  declared before the guest so it drops after QEMU is reaped. The dump is kept only when the scan
  itself fails (`Err`: duplicate or out-of-range unit, unreadable dump, pmemsave error), as the
  evidence. The rv32 ambiguity was diagnosed from such a dump. A scan that only finds a declaration
  too small still deletes it. testbench.md names the path (`<case>-<arch>-smp<N>.ram` in the run's
  directory) and the size (the guest's RAM).
- red 3: APPLIED. init.md now says what check.rs:517 refuses: zero, more than 128 pages, or a stack
  not smaller than its budget's pages, before any server starts. A budget that holds the stack but
  not the image fails that server's launch, and `init` refuses the boot there (fail-closed,
  unchanged).
- simp 1: APPLIED. `stack_at = STACK_TOP - stack_pages * PAGE_SIZE` after the 1..=128 check. The
  STACK_TOP/PAGE_SIZE+1 and usize::MAX refusal tests are kept and still refused by the cap.
- simp 2: APPLIED. `stack_tag: Option<u16>`, default None, and an untagged launch skips the paint
  (zeroed stack, as on main). Checked by grep:
  - the scanner ignores tag 0 (memory.rs `tag == 0 ... continue`);
  - the only `stack_tag` caller is init (tag i+1 >= 1) plus the client tests' tag 7;
  - launcher-orphan's untagged launches read no paint;
  - no case or host test expects untagged paint.
  Covering test: `an_image_moves_one_batch_at_a_time` now asserts the untagged stack batches are
  all zero.
- simp 3: APPLIED. `STACK_PAINT` and `stack_paint` moved to `redoubt_client::launch`; the stub keeps
  `MAX_STACK_PAGES`. A grep for STACK_PAINT|stack_paint finds no use in stub/ beyond the definition
  (the users were the client, the testbench and tests). The bench imports the paint from
  redoubt-client, which it already depended on.
- simp 4: APPLIED. The `counts` vec is gone (`!seen[i].contains(&true)`), and so is the
  `bytes % UNIT` ensure.
- simp 5: DECLINED. Both tests need ten servers so that tag 10 (0x0a) reproduces the rv32 dump's
  bytes, and a smaller fixture would need nine absent-server failures instead.
- ed 1: APPLIED at the fold (see above).
- ed 2: DECLINED. No obvious one-line pin: init-boot prints the bound under `\d+`, and the host test
  pins a formula, not the image's 507. The documented number stays.
- ed 3: APPLIED. memory-layout.md rewrapped.
- ed 4: APPLIED. userland-boot.toml's trailing blank line is restored. The servers/README.md fsd
  residual-risk edit is DROPPED, since MEM1 does not change whether fsd runs under init. The
  Connections status-line edit in the same file is kept, because it was not part of the
  instruction; say if it should go too.
- Found while reading in full, APPLIED:
  - native.md's launch-refusal list now includes the stack range;
  - native.md and init.md's launch step say a stack is painted only when tagged (init tags every
    server's) and zeroed otherwise.

## Size lines (from a local code-line counter mirroring the rule; unverified until size-budget runs)

- stub: 361 -> 362 (+1, MAX_STACK_PAGES); was 366.
- libs/client: 966 -> 987, the same as before. The paint moved in (+4) and the checked chain went
  (-4); launch.rs counts 187 code lines both at the old head and now.
- servers/init: 1952 -> 1957, unchanged.

## Read in full

- Read whole this round:
  - libs/client/src/launch.rs, libs/client/tests/launch.rs, stub/src/lib.rs;
  - tools/testbench/src/memory.rs, tools/testbench/src/qemu.rs, tools/testbench/Cargo.toml;
  - docs/kernel/memory-layout.md, docs/userland/native.md, docs/servers/init.md,
    docs/testbench.md, docs/servers/README.md.
- tests/size-budget.toml (first 60 lines plus diff) and tests/userland-boot.toml (diff) were not
  read whole.
- The other 20 recommitted files are byte-identical to reviewed 4a080b1dc and were read as diffs,
  not whole.

## Needs the grant

Before any verdict:
- size-budget, formatting, docs, client-host-tests, init-host-tests, memory-host-tests;
- the six scans;
- the whole bench at both widths.

Per-commit builds were not run.

## The 21 recommitted files, read in full (head ef40ecf3f, unchanged)

I read in full, against init.md, testbench.md and native.md as they now stand:
- Cargo.lock;
- docs/kernel/README.md, docs/kernel/budgets.md, docs/plan/m1-separation.md;
- image/README.md, image/manifest.json;
- servers/init/src/{bin/init.rs, check.rs, fuzz.rs, manifest.rs, refusal.rs},
  servers/init/tests/manifest.rs;
- tests/data/init/bound.json (long lines included);
- tests/{init-boot, init-refuses-stack, memory-host-tests, size-budget, userland-boot,
  userland-read-only}.toml;
- tools/testbench/src/case.rs, tools/testbench/src/main.rs.

That is 21 files, not 20, because size-budget.toml was also read only as a diff before.

No contradiction, and no defect. Three small imprecisions, none of them false; I have not changed
the head:
1. init.md says an image that does not fit beside its stack makes "`init` refuse the boot there".
   That holds for the first boot. On a restart the same failure reboots instead: init.rs
   `failed()` calls `reboot` when `restarting`, which is what "Restarts and reboots" already says.
2. testbench.md puts the dump "in the run's directory". Under `--sweep` the case log, and so the
   dump, is in `seed-<N>-<arch>/` inside it (main.rs `seed_dir`). "Beside the case's log" stays
   exact.
3. The description in tests/memory-host-tests.toml does not mention that the scanner refuses an
   out-of-range index at its page offset. The page and the tests do.

Code agrees with the pages:
- the manifest's declarations match the testbench.md table;
- check.rs:516-519 matches init.md's Stacks rule;
- refusal.rs's `Why::Stack` text is the line init-refuses-stack expects;
- init.rs tags each server's stack i+1;
- the bound test keeps "stack 16 + 3" (507 in budgets.md, unchanged);
- case.rs refuses `memory` outside init or with poweroff;
- Cargo.lock has the stub, redoubt-client and redoubt-sys edges the testbench needs.

The servers/README Connections edit stays, per the instruction.

## MEM2's QMP-socket fix folded (git only)

- `git cherry-pick 8bc2ace23` onto ef40ecf3f conflicted in tools/testbench/src/qemu.rs, in two
  places, because MEM2 is branched from the pre-fold head:
  1. `let qmp_path`: I took MEM2's `qmp_socket()` and kept my `Removed` drop guard on it.
  2. The measurement call: I kept my `measure_stacks` name and dropped MEM2's explicit
     `remove_file` after the measurement, because the guard already removes the socket however
     the run ends, after QEMU is reaped.
- MEM2's `qmp_socket()` (temp dir, `redoubt-qmp-<pid>-<boot>.sock`) and its host test
  `a_qmp_socket_path_binds` are taken as written, except that the return type is the
  already-imported `PathBuf`.
- I aborted the cherry-pick and folded the resolved file into the testbench commit. That commit's
  body gains: "The QMP socket lives in the temporary directory, because a run directory deep in a
  worktree makes a path longer than a Unix socket's 108 bytes, and it is always removed."
- Five commits on main 6a7d2385e; `git range-diff`: four are `=`, only the testbench commit
  changed. The tree differs from ef40ecf3f only in qemu.rs (+18/-1). Status clean.

| commit | subject |
| --- | --- |
| 2f1ce940b | client: paint each launched first-thread stack for measurement |
| da12d0766 | init: preflight each server stack before launch |
| 62ce43c83 | testbench: measure painted server stacks after boot |
| 948c0df22 | docs: describe declared server stacks and their measurement |
| 80a6662a0 | image: size first-thread stacks from six boot workloads |

New head: 80a6662a0437771067337aaafc9cf1d3e27d13fa.

- testbench.md needs no change: it says the dump goes beside the case's log and the QMP socket is
  always removed, and does not say where the socket lives.
- qemu.rs: I read the whole file earlier this session. This round I read the changed regions (the
  diff and its surroundings), not the whole file again.

Expected from the final-head scans: MEM2 measured fsd:system at 7 pages, beamlet at 17 and the
image bound at 508. If my six scans agree, the image commit's manifest.json and testbench.md's
table change, and with them budgets.md's 507 (bound 508).
