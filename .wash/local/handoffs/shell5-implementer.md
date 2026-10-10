SHELL5 handoff. Branch wp-SHELL5, worktree /home/mcloonan/redoubt/.worktrees/SHELL5. The head is 9b76ff3f9: one commit on origin/shell 41c9cbef3. Not pushed. The tree is clean.

State: the red said OK with notes; its fold (pick takes keys with any modifiers) is amended in; the branch is rebased. Gates are green: ./test-shell exit 0 (195 beamlet, 168 passed and 27 skipped on BEAM), beamlet-footprint rv64 5,894 and rv32 5,695 (limit 5,942), doccheck 0. Head sent to the orchestrator as a question; next is the merge into `shell`. Report: .wash/local/SHELL5-report.md.

Orchestrator ruling: whichever of SHELL5 and SHELL7 merges into `shell` second keeps SHELL7's lazy loading over SHELL5's registry prefix skip (Registry.declare_none).

Traps:
- Env: PATH=$HOME/.cargo/bin:$PATH, BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains, RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper, RUSTSBI_PROTOTYPER_RV32=.../riscv32imac-unknown-none-elf/release/rustsbi-prototyper.
- Footprint: make -f /home/mcloonan/redoubt/scripts/jobs.mk -C <worktree> prebuilt, then rv64/beamlet-footprint rv32/beamlet-footprint. Read target/testbench/run-*/beamlet-footprint-*.log.
- Every mix/cargo command goes through q.
- ./test-shell arguments must be files.
- beamlet-redoubt's console test flakes under load; rerun it with q --quiet.
- Keep U+202E escaped in .exs files.
