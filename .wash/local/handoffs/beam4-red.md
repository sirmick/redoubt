# beamlet red reviewer handoff (beam4-red, member c27d790b01e745343e6b1658a9d820ee)

## Branch state and traps (read first)

- I am READ-ONLY: no Write/Edit/redirects, no checkout/stash/commit; verdicts go as `answer` to the orchestrator (cab495a4d2c090f3d5882b1a537494b9), first line `Merge verdict: OK | OK with notes | BLOCK (<head>)`. Each answer must be under 2000 bytes (the tool refuses longer; trim, do not split into QA threads unless asked).
- Every build/test through `scripts/q run` (`--quiet` for host-clock cases); `make -f scripts/jobs.mk -C <worktree> rv64/<case>` for machine cases, which needs `RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper` and `RUSTSBI_PROTOTYPER_RV32=.../riscv32imac-unknown-none-elf/release/rustsbi-prototyper` exported (worktrees lack bios/).
- beamlet crates live in the `userland/otp` workspace: `cargo test --manifest-path userland/otp/Cargo.toml -p beamlet-vm|beamlet-redoubt --features fake|beamlet-screen|beamlet` (not from the root workspace, where `-p beamlet-vm` fails).
- Shell ExUnit: `export BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains; bash -c 'set -eo pipefail; . userland/shell/setup.sh; cd "$app"; mix test <files>'` from the worktree root, or `./test-shell` whole (builds beamlet; screens run on beamlet only, BEAM skips them). The cli pty test `shell_pty.rs` is `#[ignore]` unless run via `./test-shell`.
- No /tmp scratch: `/var/tmp/redoubt/<key>/`. Long waits: background + until-loop; a 3x loop under q can be killed by the 15-min background limit.
- Pinned OTP source for reading group.erl/prim_eval.S: /home/mcloonan/redoubt/toolchains/otp-28.5.0.6/lib/{kernel-*/src,erts-*/src}; vendored crates under <worktree>/vendor (miniz_oxide 0.9.1, cells at userland/native/cells).

## Where each verdict stands

- BEAM4 natives: OK with notes, merged (b25abdee5). BEAM4 cases branch: OK with notes at b1103a365 on STEWARD2's c3567f923 (tester carves as the steward: account 1001, sub-budget per label set).
- BEAM9: OK with notes (fdf31891f). BEAM10: OK with notes, merged with eof_pending fold (8894fe5bc). BEAM11: OK at 45f1880bc (busy clunk retried). BEAM12: OK with notes (6fa630ff9). BEAM13: OK with notes (28b46a346). K27: OK with notes (935afba93; steward-session-ends belongs in quiet too). BEAM15: OK with notes (c0c2dc56b).
- SHELL2: OK (9f48c4b1a). SHELL3: OK (21b9f25ac).
- BEAM16: BLOCK at 3053aae86, verdict sent. Expect a fold to renew: (P1) zlib `set_stash` copies an arbitrary term out of the heap (zlib.rs ~591) and `held()` never counts it: must count the stash's words in `held()` and declare from set_stash/clear_stash, or bound the stash; (P2) `DEFLATE_BOXED` misses `ParamsOxide.local_buf: Box<LocalBuf>` = OUT_BUF_SIZE 85,196 B (vendor/miniz_oxide/src/deflate/buffer.rs), so a deflater is undercounted ~1/3; the literal is pinned to 0.9.1 while Cargo asks 0.9: a counting-allocator host test would pin it. Renewal: check `held()` includes the stash, the new constant 168,418+85,196, and rerun `--test sized` (was 9/9) and the beamlet-vm suite.

## Open notes not yet closed (follow-ups, not blocking)

- BEAM4: a failed job hand-off destroys the caller's budget (jobs.rs `started.kill()`); `already_served` compares handle numbers only; `files.rs` original busy re-send fixed by BEAM11; a wire-wide `timeout` status (expired requests answer `malformed`).
- BEAM10/13: a reader process exiting left `cons.input` undrained (fixed by BEAM13); SSH-session EOF under a served endpoint (fixed by eof_pending).
- BEAM15: no test forces a GC mid receive-scan (a match fun allocating heavily over large queued messages).
- BEAM16/SHELL3: sized resources in ETS/persistent_term/queued messages count toward nobody; max_ets_words (1 GiB) exceeds a session budget: worth a node.
- K27: the page's quiet-class criterion should be stated as the mechanism (a session alive across the hub's 10 s hold); every long-prompt case is exposed.
- SHELL2: logger crash reports bypass Text.visible (page names it); ISIG off + Ctrl+C dropped during evaluation leaves a runaway line uninterruptible until G3.
- difftest: `erlang/ports` fails on main (/bin/echo is uutils outside the sandbox); `erlang/files2` failed once on main; atomvm suite has 6 main failures (BEAM16 report).

## Rules I hold reviews to

- Authority: a native acts only with a handle the VM was handed or minted; resources never serialisable (ETF writes a Ref; decoded grants nothing); every handle a resource term closed on last drop (Owned::drop) except startup handles; served/launched/called objects held by the platform for the operation's life.
- No VM-thread wait without a named bound (beamlet.md lists them: send 1 ms, attach 2x1 s, hand-off HAND_US 1 s); typed calls on the pool; serve through the library's admission/deadline.
- idle returns at once for anything already arrived (input, eof_pending, files.finished, sys.events) and never spins (next() consumes all three first).
- `busy` from the 9P skeleton always means not served (refused set filled before dispatch); retries after RETRY_US, never at once; a busy clunk keeps its fid.
- Sized resources: count at push_offheap, recount at GC, resize by the owner at once; check every path that changes real size declares it.
- Terminal: every byte to the terminal is the encoder's own sequence or Text.visible text; the test terminal raises otherwise; raw mode restored on main return, panic, SIGINT/TERM/HUP; alternate screen left on every exit incl. group's EXIT.
- Pages: dateless, no package IDs, status counts match tests listed, Size budget line in rt/client commits; machine-case verdicts from the system, not the host clock.
