# BEAM15 report: a `receive` typed at the shell prompt runs on beamlet

Branch wp-BEAM15, one commit c0c2dc56b on main e7f54babb. Worktree
/home/mcloonan/redoubt/.worktrees/BEAM15. Not pushed. Tier B.

## Cause and design (approved checkpoint)

erl_eval evaluates `receive` by `prim_eval:'receive'(MatchFun, Timeout)`. On BEAM, prim_eval is
not a BIF: its .erl is a nif_error stub and the real module is hand-written BEAM assembly
(erts src/prim_eval.S), preloaded; the toolchain's ebin/prim_eval.beam is that assembled code
(disassembled: loop_rec, call_fun on the message with the cursor open, is_ne_exact nomatch,
remove_message / loop_rec_end, wait_timeout, timeout; arg_reg_alloc calls bump_reductions/1).
beamlet listed prim_eval in RUNTIME_MODULES (vm/src/vm.rs), so it never loaded: undef.

Fix: prim_eval comes off RUNTIME_MODULES and loads from erts like erlang/erts_internal. No
native. Every instruction it uses is beamlet's already; the scan position (p.save) is on the
process, so a call from inside the scan keeps it; the mailbox is a GC root.

## The p.save risk (checked, and tested)

- What erl_eval passes: `fun(M) -> match_clause(Cs, [M], Bs, Lf, Ef) end`: pattern matching
  (match1) and guards. guard0 admits only `erl_lint:is_guard_test` expressions, so only guard
  BIFs run; none of beamlet's touches p.save (only REMOVE_MESSAGE, TIMEOUT, hibernate and
  demonitor's flush reset it). A guard's error is caught inside erl_eval.
- What it can still do: raise (e.g. illegal_pattern) mid-scan. beamlet does not reset p.save on
  an exception. I checked the real BEAM: it does not either (send a, b; a fun that skips a and
  raises on b; the next plain receive gets b). So beamlet already matches; pinned by the
  difftest.
- A fun that itself receives (impossible from erl_eval, possible calling prim_eval directly):
  pinned to BEAM by the difftest too (inner receive takes r, outer then removes the message at
  the reset cursor; both VMs give the same mailbox after).

## Tests

- userland/otp/tests/erlang/eval_receive.erl (difftest, beamlet vs BEAM): erl_eval receive that
  matches the second message, `after 0`, `after 20`, a guard-rejected clause then another, a
  receive with no `after` woken by a spawned sender, a bound variable in the pattern, two
  `receive Any after 0`; and direct prim_eval cases (raise mid-scan, nested receive, nomatch
  with timeout 0). Identical on both.
- userland/shell/test/redoubt/shell_test.exs, "a receive typed at the prompt takes a message,
  waits for one, or times out": on beamlet and BEAM via ./test-shell. Unfixed: fails on beamlet
  with `:prim_eval.receive/2 is undefined` (run checked).
- tests/userland-read-only.toml (machine, both widths): after the Version call, types a receive
  of a message the line sent itself (`:received`) and one that waits out `after 10`
  (`:waited`); forbids `prim_eval` and `not loaded`. prim_eval is not in the boot pack (it
  holds what the prompt loads); it is staged on the userland volume because erts.app lists it
  and image/userland.toml stages erts whole, and it loads from there on first call.
- No beamlet-vm host test: vm host tests run committed fixtures with no stdlib, so erl_eval
  cannot run there; the difftest is the erl_eval test.

## Gates (all rc=0; logs /home/mcloonan/redoubt/.tmp/BEAM15/gates/)

- prebuilt; userland-boot rv64 16.6 s, rv32 17.0 s; userland-read-only rv64 19.0 s, rv32 19.6 s;
  docs; formatting.
- cargo test -p beamlet-vm (via q): rc=0.
- ./test-shell (full): every stage passed; on_beamlet 118 tests, 0 failures.
- tools/difftest erlang: 44/44 passed, 1 skipped by design.

## B32 (image list and footprint)

The image list does not change (pack and staged applications untouched). The prompt's footprint
does not change: nothing new loads until a receive is typed. A typed receive loads prim_eval,
a 1,720-byte .beam (a handful of instructions), well under a page decoded. Note:
userland-read-only, whose post-command peak (5,475 pages on rv64) sets the session budget per
beamlet.md and testbench.md, now types two more lines; the session VM's peak is not in this
case's scan output (it scans manifest servers only), so I could not measure the change; it
should be at most a page or two. B32 should re-measure with this case as it now is.

## Pages checked

- docs/userland/beamlet.md "What runs on it": new bullet (erts's Erlang where it is code:
  prim_eval loads, its receive runs erl_eval's; the rest never load) and
  bench:userland-read-only in its status list (16).
- docs/userland/shell.md: no claim about receive at the prompt; unchanged.
- beamlet.md "What the VM holds at its prompt" / testbench.md memory budget / budgets.md: the
  5,475 figure is from the case before this change; not re-measured here (see B32 above).
- README.md, GETTING-STARTED.md, userland/otp README/DESIGN: no claim about prim_eval or receive.
