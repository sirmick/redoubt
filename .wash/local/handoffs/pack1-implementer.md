PACK1 handoff (pack1-implementer), 2026-10-06. Worktree /home/mcloonan/redoubt/.worktrees/PACK1, branch wp-PACK1, base main fa08fe2c8, NOT pushed. Detail: /home/mcloonan/redoubt/.wash/local/PACK1-report.md (read it whole).

STATE: holding for EROFS1's merge (owner decision at checkpoint 2: no littlefs target; keep 16 KiB reads; rebase onto EROFS1 when the orchestrator says, then set the target there by the Architect's rule: measured floor (boot-profile rv64 seed 1 verified prompt [t=N]) + a tenth, rounded up to 5 s; the page states one target; littlefs numbers stay as one sentence).

Commits (logical, clean):
- c41559b8a testbench: boot.pack in the stager (tools/testbench/src/userland.rs boot_pack/pack_entries/PackFault; disk.rs flip_file(.., copies); qemu.rs flips both copies of a packed file), image/userland.toml `pack = [...]` (94 names), tests/data/boot-profile/userland-unverified.toml same list
- d391de2a5 beamlet-redoubt: tests/limits.rs passes run's 7th arg (main broken since BEAM6)
- 8a818a745 beamlet: src/pack.rs reader, userland.rs Disk::with_pack, lib.rs Modules::packed + boot-stats line, bin/beamlet.rs read_pack (4-page lend, ascending), tests/pack.rs; userland-bad-start + boot-profile(-unverified) expectations
- e7f44e0e8 tests: pack-outside-module, pack-bad-{truncated,wrong-length,wrong-name} + tests/data/pack/*.toml
- 81c0856b3 docs: beamlet.md, boot.md R75, SECURITY.md R75 row, m1 progress, image/README.md

AFTER REBASE ONTO EROFS1: the stager/recipes may change (EROFS image format); keep boot.pack in the volume; rerun boot-profile + -unverified both widths, seeds 1-5 verified rv64; set the target; bound the prompt stamp in boot-profile(.toml) by it (expect regex or a bound mechanism - check what EROFS1 did); rewrite beamlet.md's boot-time sentences (Architect's page sentence in .wash/local/PACK1-implementer.md, ruling 2) and the 16 KiB read sentence if the read size/server changes (erofsd heap: check whether a 64 KiB read fits there); rerun the gate list in the report; amend the docs commit (no fix-up commits).

TRAPS:
- jobs.mk targets are substring filters: rv64/boot-profile also runs boot-profile-unverified.
- 64 KiB reads OOM littlefsd:system (heap cap 38) -> 'boot.pack not loaded: ... other'.
- Reading backwards changes nothing on littlefs (each 9P read reopens the file).
- The boot set was measured with a temporary eprintln in redoubt/src/fixture.rs (reverted); boot-stats on the machine confirms 'of them 96 from the pack'.
- Env file /tmp/pack1-env.sh (may vanish): see the instruction's env lines.
- Consoles copied to /tmp/pack1-consoles/.

AT THE REBASE (orchestrator, 2026-10-06): BEAM8 carries the same tests/limits.rs fix as its first commit and merges before EROFS1; drop d391de2a5 at the rebase unless BEAM8 has not landed by then, and say which was done.
