# B26 report: rv32 beamlet-files' rare eexist at its first make_dir

Branch wp-B26, worktree /home/mcloonan/redoubt/.worktrees/B26, head 69cb4a08b (5e0ce2422 with the
red's note folded: the case forbids the start-time line `files: removed an earlier run`, since a
first run finds a fresh volume and the end-time cleanup means a replay finds one too), one commit
on main 142f8a531 (tests/beamlet-files.toml, userland/otp/redoubt/tests/erlang/beamlet_files.erl). Tier B.

## Finding (b): beamlet did not end early

The failing run's console log was not kept (the bench keeps a run dir only for the latest runs;
mine were my two passing reruns). From the code:
- `init` restarts every server entry that exits, exit 0 included (servers/init/src/bin/init.rs,
  `restart()`: says "exited, code 0", then `self.start(i)` unconditionally; the manifest has no
  run-once key), and beamlet is a server entry in tests/data/beamlet/files.json.
- So after the case's last expected line, `init: beamlet (PID) exited, code 0`, init says
  `init: restarted beamlet, console ...` and the restarted VM replays the module on the same
  walfsd volume, which holds /home/alice/d from the first run: `make_dir` → eexist →
  `{'EXCEPTION',error,{badmatch,{error,eexist}}}`, which the case forbids.
- The bench reads a 50 ms grace after the last expect ("the rest of the output must be clean
  too", tools/testbench/src/qemu.rs:607) and judges forbids in it. Under load the bench's reader
  thread falls behind QEMU, so the replay's lines are already queued when the last expect
  matches, and the grace drains them: rare, load-only, rv32 (the slower bench process).
- beamlet-boot, beamlet-console, pack-outside-module and beamlet-heap-flood expect the same exit
  and have the same replay in their grace window; nothing forbidden is printed in theirs.
- Evidence from kept logs: before the fix, `init: restarted beamlet` inside the grace drain in 1
  of 8, then 3 of 8 kept runs (0 lines after the exit line in the others); after the fix, 2 of 5.

## Fix (approved direction, case-only)

beamlet_files.erl removes an earlier run's `big`, `d/inner` and `d` at its start (one line,
`removed an earlier run's: [...]`, only when it finds any) and its own at its end (the expected
line `files: cleaned: ["/home/alice/big","/home/alice/d/inner","/home/alice/d"]`), through
prim_file's delete and del_dir; so init's replay, and a replay after an interrupted first run,
run on a clean volume and print the first run's lines. The case's comment names init's restart of
an exited server entry and the bench's grace drain. Not done, by the orchestrator's ruling: a
manifest run-once key, a change to the bench's grace rule (both noted for the Architect).

## Reproduction and gates

Scratch: /home/mcloonan/redoubt/.tmp/B26/ (repro.sh: N bench processes from the prebuilt index,
J at a time, each `q run --cores 1`; logs per run).
- Unfixed tree (main 142f8a531), rv32 beamlet-files: 30 runs at 8 parallel, 30 PASS; 30 runs at
  16 parallel, 30 PASS. The failure did not reproduce; the restart line in the grace drain did.
- Fixed tree (5e0ce2422): 30 runs at 8 parallel, 30 PASS; no leftover was ever found (no replay
  reached the start-time check inside a run's log).
- Gates on 5e0ce2422 (jobs.mk): PASS beamlet-files rv64 and rv32, docs, formatting.

## Documentation check

docs/userland/files.md "Files over 9P" cites bench:beamlet-files; the case still tests what the
status line says, and now del_dir in a boot too: no page change. servers/init.md "Restarts and
reboots" already says a server that exits is restarted. README.md, GETTING-STARTED.md,
docs/plan/m1-separation.md: no claim touched.

## Red's note folded (69cb4a08b)

The case forbids `files: removed an earlier run`. Gates on 69cb4a08b (prebuilt rv32 204, rc 0):
PASS beamlet-files rv64 and rv32, docs. Log: /home/mcloonan/redoubt/.tmp/B26/gates2.log.
