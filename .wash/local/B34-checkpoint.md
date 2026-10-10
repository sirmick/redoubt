# B34 checkpoint: the six atomvm difftest failures

Branch wp-B34 on f94a146ed; nothing written yet. Each test was run wrapped (to print its failing
line) on beamlet and BEAM; wrappers and runner in /home/mcloonan/redoubt/.tmp/B34/.

## When

No full difftest run is on record. The six were run on beamlet built at main, at 4b074d8a2 (the
restructure), at 31a27f6d1 (beamlet's last commit in its own history) and at 891994bf3 (the
commit that brought in the AtomVM corpus). All six fail at every point and were never in SKIP:
they have failed since the corpus arrived (2026-09-18), not by regression. All pass on BEAM.

## The six

1. **test_binary_to_term** (line 298): beamlet bug. The test decodes a NEW_FUN_EXT whose MD5 is
   zero and whose index and uniq match no loaded code. BEAM keeps those fields in the fun (a
   call is `badfun`) and re-encodes the same bytes. beamlet binds the fun to the live module's
   and re-encodes with the live MD5, index and uniq. Fix: a decoded fun keeps its encoded
   identity, and is called only if it matches the loaded module, else `badfun`. Size M.
2. **test_code_all_available_loaded** (line 56): beamlet bug. The modules built into beamlet
   (beamlet_io, beamlet_code, application, ...) are `{M, loaded}` in `code:all_loaded()` but
   `{M, preloaded, true}` in `code:all_available()`. BEAM says `preloaded` in both. Fix:
   all_loaded (and which) say `preloaded` for them. Size S.
3. **test_code_server_nifs** (line 45): accepted VM difference. It calls `code_server:is_loaded/1`,
   an undocumented internal of OTP's code server that reads the code server's ETS table; beamlet
   replaces the code server (beamlet_code), so there is no table and the lookup is `badarg`.
   Proposal: SKIP, reason "calls OTP's code server internals; beamlet's code server is its own".
   The alternative, a native for that one function, I would not do.
4. **test_display_string** (line 36): beamlet bug. `erlang:display_string(standard_io, _)` prints;
   BEAM takes only `stdout` or `stderr` and raises `badarg`. Fix in the native. Size S.
5. **test_node** (line 36): accepted VM difference. It decodes a pid, a ref and a port of another
   node (`test@test_node`), which beamlet refuses (no distribution): the reason `test_ets` is
   already in SKIP. Proposal: SKIP with that reason. The test's local half (`node()` of self(),
   of a ref, the `badarg` cases) loses its coverage unless it is moved into an erlang suite case.
6. **test_unicode** (line 37): beamlet bug. `unicode:characters_to_list([$h, -1], latin1)` gives
   `{error, "h", [[-1]]}`; BEAM gives `{error, "h", [-1]}`: the rest gains a list level. Fix in
   the unicode native's rest, checking the other rest shapes the test asserts. Size S.

## The gate rule

The node asks that the full difftest join the gate list for any change under userland/otp.
Who records that rule: SWARM.md, or docs/userland/beamlet.md's "What runs on it"?
