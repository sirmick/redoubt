# B29 report: rv64 beamlet-serve times out on main 7743587e2

Branch wp-B29, worktree /home/mcloonan/redoubt/.worktrees/B29, head 964f15731, one commit on main
7743587e2 (tests/beamlet-serve.toml, tests/beamlet-programs/src/bin/beamlet-caller.rs,
userland/otp/redoubt/tests/erlang/beamlet_serve.erl). Tier B (a case and its test programs).

## Bisect (fresh worktree per commit, jobs.mk, rv64 beamlet-serve)

- 91256b4d0 (WFS2 merge, B27's base): FAIL 2/2, differently: `init: refused the boot:
  beamlet-session: could not make a fresh connection for ...` — the pre-B27 state, which B27
  fixed (the tester is handed keyd, which mints no connection).
- c144f28fc (SHELL2 merge): FAIL 2/2, the same pre-B27 refusal.
- 7743587e2 (main, B27 merged): PASS 2/2 serially, then PASS 12/12 at 6 parallel.
So neither SHELL2 nor B27 breaks the case; the orchestrator's 4/4 failures were beside other
members' gates, and their kept console logs (train-11, run-1166967-*, run-1173105-*,
run-1176948-*, run-1182120-*) show every expected line present but the caller's
`beamlet-caller: 41 answered 0 42` BEFORE the VM's `serve: request: ...` and `serve: reply: ok`.

## Cause

Two programs write two console connections: the VM's `request:` line is an io:format whose hub
write can land after the VM has replied and the caller has printed. The bench's `expect` list is
one ordered list (docs/testbench.md: "each must match a console line, in this order"; no
unordered form), so after it matched `request:` it waited for a caller line already gone by, and
timed out at 180 s. Load makes the inversion the rule, not the exception.

## Fix

The caller makes a third call (opcode 43) after the serve thread's deadline has answered its
second, carrying both answers' words; the VM receives it, says `serve: caller: 41 answered 0 42,
42 answered 1`, replies, and says `still serving after the deadline` (the third request's arrival
is that proof, in place of the 6 s sleep). Every judged line is the VM's, in its own order; the
caller's own lines stay for a reader, unexpected; `beamlet-caller: .* refused` is forbidden, and
the tester's `beamlet-caller ended: Exited, code 0` still closes the case (the caller exits 0 only
if its report was answered). No bench change, no VM change (./test-shell not run for that reason).

## Gates on the committed tree's content (prebuilt rv64 229 / rv32 216, rc 0)

- rv64 beamlet-serve: PASS 2 serial + 12 at 6 parallel + 1 in the gate set = 15 of 15; rv32 2/2.
- PASS both widths: beamlet-natives, beamlet-launch, beamlet-natives-attack, beamlet-files.
- PASS docs, formatting.
The gates ran on the worktree before the commit; the commit changed no byte of the tree.
Logs: /home/mcloonan/redoubt/.tmp/B29/ (bisect chains, fix-first, fix-load, fix-gates).

## Documentation check

docs/userland/beamlet.md "Natives" cites bench:beamlet-serve; what the case shows is unchanged
(serve/1, reply/2, the caller's badge/account/labels, the deadline's malformed answer): no page
change. The bench page's rule (ordered expects) stands and is what the case now obeys.
