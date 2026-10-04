# BEAM2 handoff 3 (beam2-implementer-3, 2026-10-03)

Brief: /home/mcloonan/redoubt/.wash/local/BEAM2-implementer.md (rulings 1-5 at its end). Detail
of everything done and measured: .wash/local/BEAM2-report.md in this worktree (read its last three
sections). Worktree /home/mcloonan/redoubt/.worktrees/beam2, branch wp-beam2, clean tree.
Every cargo and bench command: `/home/mcloonan/redoubt/.wash/local/in-dev <command>` from the
worktree.

## Tip e31155894 on main 75245a114 (five commits, each final; no fixups pending)

- 9f9a7f566 init: each volume names its disk, and its range is minted at that disk's blkd
  (`check::blkd`, `check::on_disk`; size: init 1917 -> 1936)
- 34b5bb65d testbench: pack the userland disk, its objects named by their hashes, and
  system.index (image/userland.toml with erts; optional `stage` for a userland recipe; init
  1936 -> 1937)
- b9a07f8df beamlet: modules from the userland disk, each checked against system.index (R75;
  beamlet.md's confined line; docs/todo/beamlet-refused-module-falls-through.md; also carries
  the simplifier's packer map: `Names`, `Staged`, `Builder.staged`)
- be50bfc62 blkd: receive on the endpoint its endpoint= argument names (`parse_args` ->
  `Args{endpoint, labels}`; init's `Why::BlkdEndpoint` check and its host test; every manifest's
  `endpoint=`; blkd 1606 -> 1627, init 1937 -> 1951)
- e31155894 image: the shell on the UART, its modules from the userland disk (manifest disk1,
  blkd:system, fsd:system, beamlet 24,576 pages; RecipeEntry `workspace`; memory_mib 1024 cases;
  userland-boot, userland-bad-start, userland-read-only; fsd-client `readonly`; init 1951 -> 1952)

History tool when a fold is needed: `git commit --fixup=<sha>` (or `--fixup=amend:<sha>` with
GIT_EDITOR="cp msgfile", message starting `amend! <subject>`), then
`GIT_SEQUENCE_EDITOR=: git rebase -q -i --autosquash 75245a114`. Size-budget ceilings live on
one line per crate: fixups to that line conflict across commits; replay instead (the script used
was /tmp/beam2-ladder.py: cherry-pick -n each commit, take its size-budget.toml, set the line).

## Reviews of 1c92d78f4: all folded

- Editor: (1) four rewraps; (2) size lines in CONTRIBUTING's form with counts. Folded. Its note
  that a7d98e3da carried a size line was mistaken (the line was the blkd commit's); told.
- Simplifier: 1 packer map, 2 blkd `Args`, 3 `on_disk` folded; 4 kept the four beamlet JSONs
  (BEAM1's every_beamlet_is_told_its_own_budget reads JSON only), `stage` made optional;
  5 mkimage double staging: report note only.
- Red: 1 `Why::BlkdEndpoint` + test + init.md clause; 2 read-only description trimmed to fsd's
  refusal; 3 vm.rs `locate_module` (~:869) falls through on a refusal: todo page for BEAM3/BEAM4
  (docs/todo/beamlet-refused-module-falls-through.md, in SUMMARY.md), no code change.

## Owed (needs the host; the orchestrator says when; one case at a time)

Before each: `pgrep -af qemu-system | grep -v defunct` empty and no `testbench` without a case
name running. Each `in-dev cargo testbench <case> --arch rv64|rv32`.
1. userland-boot both widths (its current form, flip Version + remove OptionParser, has never
   run); init-boot and image-disk both widths (new expects: 10 / 11 servers, 6 badges, 8
   consoles; never run with the shell in the image); userland-bad-start and userland-read-only on
   rv32 (rv64 passed). Earlier forms passed: userland-boot rv64 214 s, rv32 128 s.
2. userland-boot's time alone and in a whole run, both widths; set its `timeout_secs` (now 400)
   to twice the whole-run time (ruling 5); same consideration for bad-start/read-only.
3. If a case fails, fold the fix into its owning commit.
4. `git rebase --onto 53bcd9704 75245a114 wp-beam2` (never plain rebase main); then the host gates
   again: size-budget, init/blkd/fsd-host-tests, docs, no-cruft, unsafe-budget, vendor-check,
   `cargo test -q -p testbench`, `cd userland/otp && cargo test -q -p beamlet-redoubt --features
   fake`, fmt --check (root and userland/otp), release builds rv64/rv32 of init, blkd,
   fsd-programs. Size ceilings may need re-measuring after the rebase (other packages move them).
5. The Architect's check, then the whole bench both widths (alone), then the final report and
   completing the assignment (7e00340128cc292ddd330e3e4d7110a7).

## Measures to report (final)

587 objects = 579 modules + 8 .app; 7,837,721 bytes stripped, 9,310,929 with Docs; excluded
application.beam, gen_tcp.beam, ram_file.beam; 14 erts RUNTIME_MODULES on disk never loaded.
VM use at the prompt: rv64 11,877 pages, rv32 7,554; budget 24,576. Not yet re-measured: the
idle prompt's closure with uncompressed objects (could print `length(:code.all_loaded())` in a
scratch boot; optional).

## What consumed context (avoid)

- Reading whole files to commit them (init tests 1,000 lines, case.rs, qemu.rs): needed once;
  they are read and committed now.
- `git diff` of many files at once (32 KB); diff one file at a time.
- Shell measurement through a temporary init patch (.wash/local/BEAM2-measure-init.patch, kept):
  not needed again unless budgets change.
- The full init manifest test runs printing every failure: grep `panicked|left:|right:` only.
