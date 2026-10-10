# Environment and rules for every member launched 2026-10-09

Main is f5316e38c (B45 merged). The workspace was restarted today; every member before it ended
without a handoff unless `.wash/local/handoffs/<key>.md` says otherwise. Each open branch's state
is in git: read `git log main..wp-<PKG>` and the diff before anything else, and report what you
found within your first hour.

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
