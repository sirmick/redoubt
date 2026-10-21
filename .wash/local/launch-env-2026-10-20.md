# Environment and rules for every member launched 2026-10-20

Main is 4750dd61c (train 92: B101, the kernel lock wakes only the next ticket). Your branch `wp-<PKG>` starts there in `.worktrees/<PKG>`.

## Shell

```sh
export PATH=$HOME/.cargo/bin:$PATH
export BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
export RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
export RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
export REDOUBT_TMP=/home/mcloonan/redoubt/.tmp
```

In every shell, before any build or case. A fresh shell without them fails every boot case in
0.0 s ("RustSBI Prototyper not found").

## The machine scheduler

Every cargo build, host test, difftest, QEMU boot and bench case runs through `scripts/q`:

- `/home/mcloonan/redoubt/scripts/q run --cores N -- <cmd>` (N = the harts a boot uses, 4–8 for
  a build; `--lock net` for a case that binds the loopback sshd port);
- `make -f scripts/jobs.mk set CASES="a b c"` runs those cases on both widths, each leasing for
  itself; `make -f scripts/jobs.mk prebuilt` builds once for the cases; `docs` the docs check.
  `scripts/shell-cases` picks the shell set. `scripts/q ls` shows the queue.

Never a bare `cargo testbench`. Scratch only under `$REDOUBT_TMP/<PKG>/`, never `/tmp`.

Do not edit anything in your worktree (docs included) while `make prebuilt` or any case runs:
they fail "tree changed". Commit, prebuild, run, then edit; set edits aside as a patch under
`$REDOUBT_TMP` if you must.

## Context

- Never read a `.wash/qa/*.md` file whole or `tail` it: a base64 checkpoint blob ends each one
  (100 KB+). `grep -n '^## ' file` and `sed -n` the one event you need.
- Grep logs (`grep -n 'FAIL\|PASS\|panic' log`), never `cat` them; no `./test --list`; no
  boot-log hex dumps into your context.
- Read a file's region, not the file, when you know the region.

## Git

- Stage by path. Never `git add -A`, never `git stash`, never push.
- Your `wp-<PKG>` branch is yours to rewrite: amend and reword into clean logical commits; no
  fix-up commits; a commit message never mentions what the commit does not contain.
- Commit messages end with `Co-Authored-By: <your model's name> <noreply@anthropic.com>`.
- A raised size or unsafe ceiling needs a `Size budget: <crate>: <reason>` (or `Unsafe budget:`)
  line in the commit that raises it; `size-budget` refuses it otherwise.
- The model generator (`model/src/gen.rs`): never add RNG draws for a new call; pick from values
  already drawn. Extra draws shift every seed and break `model-mutations`' deadlines.

## Reporting

`member_update` with a result of at most 2000 bytes; detail in `.wash/local/<PKG>-report.md`
(gates as run with their exit codes, base and head commits, measured numbers, affected summaries
checked). Then set waiting and end your turn. Never poll.

## Reviewers

You never write. No `Write`, no `Edit`, no shell redirection (`>`, `>>`, `tee`), no git command
that changes anything (no add, commit, checkout, stash, reset, push), no files under
`.wash/local`. Your report is your assignment result: first line the verdict, then base and head
commits, the paths read, findings numbered P1/P2/P3. Read in the implementer's worktree
(`.worktrees/<PKG>`) with `git diff -U0 main...<head>`, `git show`, grep and `sed -n`.

## Traps learned since 2026-10-09

- Scratch (`$REDOUBT_TMP`) must sit outside your worktree: an untracked directory inside it counts
  as a tree change and fails prebuilt cases.
- Never `git worktree move`; for a measurement on another commit, `git worktree add --detach` a
  scratch worktree under `$REDOUBT_TMP/<PKG>/` and remove it when done.
- A 0.1 s host-test or docs failure naming a missing top-level file or an old path is a stale
  binary, not your change: `cargo clean -p <crate>` and rerun.
- Under icount (most kernel cases) placement and phase shift with any change in kernel cost: a
  case that moves deterministically by a few per mille or loses its witnesses is first suspected
  of measuring QEMU's turn order (LAT4, K31's rulings in docs/kernel/scheduling.md 'A hart waits
  for QEMU's turns'); show it with a run where harts run at once (an -mttcg twin) before changing
  a bound, and never widen a tolerance without a ruling.
- Ask `q` for 4 cores per job; a 16-core request waits behind the bench for half an hour.
- Never `pgrep -f`/`pkill -f` a pattern you typed: select by `/proc/<pid>/cwd`.

## Traps learned since 2026-10-12

- The docs and formatting gates are the bench cases: `make -f scripts/jobs.mk set CASES=docs` (C11:
  no package ids in cases or comments) and `set CASES=formatting` (nightly fmt, userland/otp and
  every .ex/.exs). A plain `mdbook build` or `cargo fmt --check` is not the gate.
- A changed `libs/rt` contract is built and tested against every bin linking `redoubt_rt`
  (tests/net too) before any wider run.
- A console line a case expects is a contract: grep `tests/*.toml` before moving, splitting or
  suppressing any steward/kernel/server line.
- `make ... set` reruns overwrite `target/jobs/<width>-<case>.log`: copy a failing log aside to
  `$REDOUBT_TMP/<PKG>/` before rerunning.
- Bench run directories (`target/testbench/run-*`) are pruned to the newest few: copy a console
  log you need right after the run.
- Timing cases judged under load: a failure that passes alone is rerun alone before it is called
  a finding; say which.

## Load runs are serialized (wave of 2026-10-20 evening)

Several members run in this wave. Any run that loads the machine on purpose (a whole bench, two
whole-class sets, a repeating boot set of 8 or more jobs, or anything asking `q` for more than 8
cores) waits for the orchestrator's go: send a progress note "load run: <what>, <jobs>, ~<min>"
and set waiting; you get an instruction when the machine is yours. Single cases and small sets
(at most 4 jobs) need no go.

## A change to image/manifest.json changes the boot profile's copy too

tests/data/boot-profile/manifest-unverified.json must equal image/manifest.json less its
verification (servers/init/tests/manifest.rs). Any edit to an image entry (heap, stack, args,
weight) is made in that copy in the same commit, and `init-host-tests` is in the gate. Two trains
failed on this.
