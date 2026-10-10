CONS1 handoff (b30-implementer, at 70% context)

## Branch state and traps first
- Worktree /home/mcloonan/redoubt/.worktrees/CONS1, branch wp-CONS1, clean tree, on origin/main 8fa39c5ad (B44). Never pushed.
- Commits, in the order the orchestrator set:
  - (a) 953f5173e rt: typed parking (TypedServer::waits default false, typed::answer_or_wait/serve_parking/reply; NineServer::serve_parking/serve_with `own` returns Result<Option<Request>,Error>; run()'s own keeps its old signature via the Own adapter). All 15 callers updated. Size budget lines: libs/rt, servers/sshd.
  - (b) 182d97137 consoled, sshd: consol size/resize via new libs/rt/src/server/consol.rs (Consol protocol, Now dispatcher, asks(), serve(), reply_resize()). consoled `size=COLS,ROWS` arg (size_arg, 1..1024, default 80x24, bad -> BAD_LIMITS); parked state enum Waiting{Read,Resize}. sshd Chan.resized counter + size()/resize_due(); slot turn answers due resizes before re-serving Io. Docs consoled.md/sshd.md/serving.md. Size budget lines rt/sshd/consoled.
  - (c) 5b78cf814 beamlet: Platform::console_resized (default None); vm.rs poll_console sends {beamlet_console_resize,{C,R}} to the reader before input; userland/otp/redoubt/src/resize.rs: a thread started by the first console read (start_reading) loops consol resize, stores the size in an AtomicU32, wakes the VM with badge 0x200; dispatch takes it; idle returns while listening and pending. Fixture console serves consol; fixture::console_channel (in_flight 4, sshd-like); ConsoleServer::resize()/resizes_waiting(). VM test + fixture .beam rebuilt (erlc +deterministic). beamlet.md table/rows/tests.
  - (d) 9a52e071d shell: driver handles {:beamlet_console_resize,{c,r}} (size fn replaced, Term.resize; screen in front gets {:resize,c,r}; else an open line is redrawn via Term.request(:redraw_prompt)); Redoubt.Screen loop resizes its Buffer on {:resize}. Tests in driver_test (2) and screen_test (1, beamlet only). shell.md new section "The console's size" (anchor #the-consoles-size; old anchor links updated), planned section renamed "Paste, scrolling and a plainer terminal"; m2 progress line.
  - (e) e8a4ed7db **WIP**: tests/consol-size.toml + tests/data/init/consol.json + tests/init-programs consol-client bin (Out.console now pub field), tests/steward-ssh-resize.toml, status lines adding bench:consol-size / bench:steward-ssh-resize (serving.md, consoled.md, sshd.md, shell.md). **Must be reworded into a real commit** (no WIP may merge): message along "tests: consol-size and steward-ssh-resize, on both widths". consol-client.rs not yet rustfmt'd (run rustfmt +nightly --config skip_children=true on it; Cargo.toml registers it with test=false).
- TRAPS:
  - consoled's in_flight is 2 per bucket, so a lone connection's share is 1, and a multiplexed session's completion call takes it: the VM's resize on consoled is refused (malformed) and the thread ends. This is by design and documented (consoled.md, beamlet.md, shell.md). Only sshd (in_flight 4) delivers resizes. The fixture's console_channel is sshd-sized for that test.
  - Session expect patterns: a typed command's echo contains its text. Match frame-only text ('Enter choose') or line-end anchors ('"one"$', '\{100, 40\}$').
  - Any edit makes target/prebuilt stale; rerun `make -f scripts/jobs.mk prebuilt` before cases.
  - The rt contract gate: build every redoubt_rt consumer before any machine run: `q run -- cargo test --workspace --no-run` (passed after the rebase) and cargo build --release --target riscv{64gc,32imac}-unknown-none-elf for servers (sshd, consoled, ipd, init-programs passed). Note `./build --programs` builds only test programs, not servers. Host `cargo check --workspace --all-targets` fails on tests/programs (target-only, pre-existing).
  - Use GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash for fixups (worked).
  - Rebasing size-budget.toml conflicts: main moves ceilings; recompute with the size-budget case.

## Gates so far
- Host: rt, consoled (12/12 x12 runs after a fix), sshd, client (file.rs passes in full now: B43 fixed on main 8e50b5489), beamlet-redoubt --features fake, beamlet-vm: all pass (run before the last rebase onto 8fa39c5ad; rerun after).
- ./test-shell on driver_test + screen_test: pass both VMs. **Whole ./test-shell not yet run** after (d).
- Bench on 8fa39c5ad base: consol-size rv64/rv32 PASS, steward-ssh-resize rv64/rv32 PASS, beamlet-footprint rv64/rv32 PASS.
- Footprint (B44 lowered session to 11,008; cap 10,989, half 5,494): main 8fa39c5ad 5,459 rv64 / 5,277 rv32; with CONS1 5,462 / 5,279 (+3 / +2 pages). Fits; no size raise needed.
- size-budget PASS after (b) (rerun at the end); docs PASS; formatting not yet run as a case.

## Next
1. rustfmt consol-client.rs; reword WIP (e) into a clean commit.
2. Rerun the host tests on the final tree; whole ./test-shell (3x is not required; once).
3. Gates: the beamlet set + steward-*/sshd-* cases + consol-size + steward-ssh-resize on rv64 and rv32 (`scripts/shell-cases` gives the beamlet set; add steward-*/sshd-* by glob), beamlet-footprint both widths, size-budget, unsafe budget, docs, formatting (bench formatting case and mix format).
4. Report to the orchestrator as a question with head and gates; Tier A: the steward red reviews. Detail file: .wash/local/CONS1-report.md (not yet written). Design note: .wash/local/CONS1-design.md.
- Scratch: $REDOUBT_TMP/CONS1 (.tmp/CONS1); no scratch worktrees left.
