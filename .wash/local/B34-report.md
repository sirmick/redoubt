# B34 report: the six atomvm difftest failures

Branch wp-B34 on main f94a146ed, head f17dc223b, eight commits. Worktree
/home/mcloonan/redoubt/.worktrees/B34. Not pushed. Findings and the when-it-began bisect:
.wash/local/B34-checkpoint.md (all six failed since the AtomVM corpus arrived, 891994bf3; never in
SKIP; no regression).

## Commits

1. c1cb1be3b beamlet: display_string/2 takes only stdout and stderr. Pinned by
   atomvm/test_display_string.
2. 83b33ea4d beamlet: a unicode conversion's rest is shaped as BEAM shapes it. collect() rewritten:
   each enclosing list's remaining tail on a stack; a cut UTF-8 sequence takes its bytes from the
   following binaries, incomplete only if the input ends first; characters_to_binary/2 takes [Bin]
   as Bin. erlang/unicode_rest: ~70 shapes probed on BEAM. atomvm/test_unicode passes.
3. d3f09ffee beamlet: a loaded module's file is one answer across code's queries (which,
   is_loaded, all_loaded, all_available; a module loaded from the code path records its path).
   erlang/code_files. atomvm/test_code_all_available_loaded passes.
4. cbcde84b5 difftest: test_node and test_code_server_nifs skipped (no distribution; OTP code
   server internals), test_node's local checks in erlang/node_local.
5. 616dd5dce (Tier A) beamlet: a decoded fun keeps an identity the loaded code does not have.
   New heap kind FunDecoded with External {md5, old_index}; FunView::Local.external; a fun with the
   loaded module's checksum is that code's fun (OldIndex not identity, as BEAM); any other is kept,
   written back byte for byte, a call is badfun, compares unequal. erlang/fun_identity + host test.
   beamlet.md states the two residuals (order of funs differing only by checksum; BEAM reusing the
   first decode's OldIndex) as known differences.
6. 61d2d898f (Tier A) beamlet: binary_to_term [safe] refuses an export fun of code not exported
   (Loaded view: checksum + exported; System::term_decoding splits the borrows, loads nothing).
   Safe mode also decodes no local fun: the orchestrator's decision B, stricter than BEAM, stated
   on beamlet.md ("Safe decoding names no code") with the comment in etf.rs pointing to it.
   erlang/safe_export (11 cases = BEAM, with and without safe) + host tests
   safe_mode_refuses_an_export_fun_of_code_not_exported and safe_mode_decodes_no_local_fun.
   The "opposite pin" for local funs is a host test, not a safe_export line: the difftest holds
   beamlet to BEAM, which accepts.
7. 30366d30d difftest: test_binary_to_term skipped (other nodes' pids/refs/ports, BEAM's <0.1.0>,
   a port without --exec, a socket); its other checks in erlang/binary_to_term_local (AtomVM
   Apache-2.0 header kept; three marked departures: module name bytes, safe local fun, other-node
   pid). They found one more: a fun's creator pid of another node did not decode; BEAM takes any
   node's (the fun does not keep it); etf.rs creator() does now.
8. f17dc223b swarm: a change under userland/otp gates on the whole differential test (.wash/SWARM.md,
   running a package, step 3). NB: .wash/README.md lists SWARM.md as owner-written; added as the
   orchestrator directed; the owner may want to see it.

## Gates (logs /home/mcloonan/redoubt/.tmp/B34/gates/)

- tools/difftest, every suite: 527/527 passed, 21 skipped by design (rc 0).
- cargo test -p beamlet-vm (via q): rc 0.
- jobs.mk rv64/formatting rc 0; rv64/docs rc 0.

## Pages checked

- docs/userland/beamlet.md: "Loading hostile code" (safe-mode rule; status list +2 tests, 12) and
  "What runs on it" (fun identity and its residuals). No other page mentions binary_to_term safe
  mode, fun identity, display_string, unicode rests or code:all_loaded.
- README.md, GETTING-STARTED.md, userland/otp README/DESIGN: no affected claim.

## Open risks

- Fun identity: a FunDecoded fun whose module is loaded later with the same checksum compares
  unequal to that code's own funs (BEAM: equal). Rare; not pinned.
- binary_to_term_local is a copy: AtomVM changes to its original are not followed.
