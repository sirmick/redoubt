# INIT2 handoff 4 (init2-implementer-4 → next)

## State

Branch `wp-init2`, worktree `/home/mcloonan/redoubt/.worktrees/init2`, tip `84dda1274`, on
`d974e247c` (wp-init1's tip; not rebased: INIT1 not merged). Tree clean: everything is committed
in logical commits, NO WIP left; the WIP split is DONE. Full detail of every commit, case and
page: `.wash/local/INIT2-report-4.md` in the worktree (read it, it is the report draft).

A whole `cargo testbench --allow-skip` was started at the tip (output /tmp/init2/bench.out, lost
on reboot): at handoff 70 PASS, 1 SKIP (bench-ssh-loopback-openssh, podman), no FAIL so far. If it
is gone, rerun it. Already green at the tip: nightly fmt --check 0, doccheck 0, size-budget and
unsafe-budget PASS, `cargo testbench init` 44 PASS (every init case both widths, init host tests,
builds), bench-init-reporter-forged PASS.

Commits after the base: e4cec8e57 (docs cherry-pick, drops at rebase), 0a5c6cbdb d1, 4f669c9a7
.. 1fb7dd477 (unchanged), 358ffa8f4 GPT (+ its testbench.md line), 02a6f6574 init: check refuses
a manifest the boot could not run (WIP lib half), 03972feb0 init: the boot starts the servers
(WIP program half + root-vs-bound check at the end of the boot), d3dc01e79 refusal cases,
9974c7e5f rt minted id, b87c9d086 consoled prefixes, 8ba51011f consoled bucket state,
82e9468c6 servers' cases under init, 84dda1274 mkimage recipe.

## d3 (done; QA INIT2-con-id answered (a) x3: resolve it with these commits as evidence)

- rt 9974c7e5f: `Ticket::id()`, `FileServer::minted(caller, badge, id, root, quota)`; others
  take `_id`; host test `the_file_server_learns_the_id_the_requester_got`.
- consoled b87c9d086: `server::Lines` (sink-agnostic; the open line, a prefix cut short is
  finished first), `[con N] ` with N = id in 16 lowercase hex digits; init announces
  `init: started NAME, console {id:016x}`; host tests tests/lines.rs (4). Check refuses a
  `handed` item naming an endpoint a consoled receives on (Why::ConsoledRoot "only init holds a
  root badge at consoled"), host test + bench init-refuses-consoled-handed. consoled.md says so.
- consoled 8ba51011f: per-bucket state = MAX_THREADS (doc gives why; BUDGET at 31: 7 buckets,
  at 255: 5); host test init_s_badge_mints_a_console_for_every_server_init_can_start (account 0).

## d4 (done)

15 refusal cases + public-manifest + consoled-handed, both widths, from image/manifest.json
changed one way each (generator was /tmp/init2/gen.py, gone; the JSON is committed). init-boot
checks root after boot (`root holds N pages for it, within the bound of B`). init-servers
(boot-reader reads /boot's greeting; reporter by [con N]); init-console-forgery (ruling 6);
bench-init-reporter-forged (must_fail). Test programs: crate tests/init-programs
(redoubt-init-programs), listed uncounted in unsafe-budget.

## d5 (done)

image/boot.toml is the recipe; testbench `recipe =` in a boot case and `--recipe` for --run;
mkimage uses it (its --program option removed); init-boot boots the recipe; host test
the_image_recipe_packs_init_the_servers_and_the_manifest.

## d6 (done)

Every ruling-3 page moved, each in the commit with its tests (see the report).

## Next steps

1. Confirm the whole bench (one SKIP), then member_update the report (summary < 1900 bytes, tip,
   path of INIT2-report-4.md) for the review round; resolve QA INIT2-con-id.
2. When told INIT1 merged: rebase onto main (e4cec8e57 drops), rerun gates.

## Traps

- Size budget: each commit that grows a crate needs its own `Size budget: <crate>: <reason>`
  line, ONE line. rustfmt wrapping a signature counts (ipd +7).
- Edition 2021 in tools/testbench: no let-chains.
- `cargo fmt` must be `cargo +nightly fmt`.
- TOML literal strings cannot hold `'`; use a basic string.
- consoled allows a badge with an account only a share of the bucket; init is account 0.
- Hunk staging without stash: `git apply --cached` of chosen hunks; whole-file staging of an
  intermediate version via `git hash-object -w` + `git update-index --cacheinfo`.

## Context accounting (implementer 4)

Before the first tool call: ~20K tokens (system prompt and tool list ~12K, the two inbox
messages ~3K, then SWARM sections + handoff 3 + brief d3-d6/rulings ~8K in the first calls).
Largest outputs since: reading servers/consoled/tests/consoled.rs whole (~7K), the WIP's
check/bound/refusal diffs during the split (~6K), inbox_read returning the full first
instruction again (~3K); then init.rs ranges (~5K total).
