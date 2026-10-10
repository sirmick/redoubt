B34 handoff (b31-implementer). Branch wp-B34, worktree /home/mcloonan/redoubt/.worktrees/B34, base main 3fef5d657. Head d7c15f507. Worktree clean. Nothing pushed. With the beamlet red (Tier A commits 67a4ca460, a5f5db6bf). Report: .wash/local/B34-report.md (written for the pre-rebase head f17dc223b; the SWARM.md commit was dropped at the orchestrator's request, who puts the rule to the owner). Checkpoint: .wash/local/B34-checkpoint.md.

Commits (3fef5d657..d7c15f507):
- 0328e4653 beamlet: display_string/2 takes only stdout and stderr (bif/info.rs).
- 2036e523d beamlet: a unicode conversion's rest is shaped as BEAM shapes it (bif/unicode.rs collect(); characters_to_binary takes [Bin] as Bin) + tests/erlang/unicode_rest.erl.
- 8540876aa beamlet: a loaded module's file is one answer across code's queries (bif/code.rs loaded_file(); info.rs; vm.rs System::module records a code-path module's path) + tests/erlang/code_files.erl.
- 1aecd2a73 difftest: test_node, test_code_server_nifs skipped (tests/atomvm/SKIP) + tests/erlang/node_local.erl.
- 67a4ca460 (Tier A) beamlet: a decoded fun keeps an identity the loaded code does not have (term/mod.rs Kind::FunDecoded, External{md5,old_index}, FunView::Local.external, Heap::fun_decoded; etf.rs; interp.rs fun_entry checks md5; cmp.rs; proc.rs fun_info; beamlet.md fun-identity bullet with residuals) + tests/erlang/fun_identity.erl + host test.
- a5f5db6bf (Tier A) beamlet: binary_to_term [safe] refuses an export fun of code not exported; safe mode decodes no local fun (decision B: stricter than BEAM) (etf.rs Loaded{md5_of,exported}, EtfError::NotExported; vm.rs System::term_decoding(); erlang.rs; beamlet.md 'Safe decoding names no code', status 12) + tests/erlang/safe_export.erl + host tests safe_mode_refuses_an_export_fun_of_code_not_exported, safe_mode_decodes_no_local_fun.
- d7c15f507 difftest: test_binary_to_term skipped; its other checks in tests/erlang/binary_to_term_local.erl (3 marked departures); etf.rs creator(): a fun's creator pid of any node decodes.

Gates on d7c15f507: tools/difftest every suite 527/527 (21 skipped by design) rc 0; cargo test -p beamlet-vm rc 0. Formatting and docs were rc 0 on the pre-rebase head (same tree but the dropped SWARM line).

Next: apply the red's notes if any (fold into the commit each concerns: git commit --fixup=<sha> then GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash 3fef5d657), rerun difftest every suite + cargo test -p beamlet-vm + jobs.mk rv64/formatting rv64/docs, send head as a question.

Traps: env PATH=$HOME/.cargo/bin:$PATH BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains (+RUSTSBI_PROTOTYPER*); everything via /home/mcloonan/redoubt/scripts/q run or jobs.mk; cargo +nightly fmt in userland/otp before commits; gate script .tmp/B34/gates.sh (difftest + vm tests); single atomvm test: .tmp/B34/runb.sh <test>; BEAM erl at /home/mcloonan/redoubt/toolchains/otp-28.5.0.6/bin/erl; the difftest holds beamlet to BEAM, so a deliberate difference is pinned by a host test, not a difftest line; read committed files in full; no git add -A.
