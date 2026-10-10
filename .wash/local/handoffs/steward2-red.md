# steward2-red handoff (2026-10-08)

Role: red-team reviewer (Redoubt). Read-only: never Write/Edit/redirect, never stage, commit, checkout or touch a worktree's files. Verdict first line of every answer. Answers to the orchestrator are capped at 2000 bytes: split long verdicts into (1/2), (2/2) messages. Set waiting after every report; never poll.

## Shell setup for runs
export PATH="$HOME/.cargo/bin:$PATH" RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
Every build/test through scripts/q run --cores N --tenant <node>-red -- <cmd>, or make -f scripts/jobs.mk -C <worktree> rv64/<case>. Scratch under /var/tmp/redoubt/<node>-red (never /tmp, never the worktree root). If jobs.mk says "built from this tree before it changed", run `prebuilt` first (~4 min). Erlang for Elixir probes: toolchains/otp-28.5.0.6/bin on PATH.

## Open / renewals owed
- CTX1a (wp-CTX1): verdict OK with notes delivered on 922a7c7e5; worktree already at a624f8c92 (patch-identical rebase onto 5f66a4963 = SHELL10's merge). If asked to renew: `git range-diff 415c86ad6..922a7c7e5 <newbase>..<head>` must show "=" for both commits; then renew by reference.
- SHELL4 (wp-SHELL4 e6e821586): OK with notes, P1 OPEN: completer.ex parameter/1 uses Code.Fragment.container_cursor_to_quoted(head), which takes no existing_atoms_only (fragment.ex:1273) and interns typed identifiers as atoms; verified with the toolchain's elixir. Expect a fix round: check the fix resolves the call's name via to_existing_atom or parses a head whose identifiers already exist, with a test like the :mod.fun one. Editor's BLOCK on help/terminal.md:20 (stale "not yet guarded") concurred.
- SHELL6 Files (wp-SHELL6-files 14d5d2766): OK with notes, P1 OPEN: save/3 — IO.binwrite raises on a write error and File.open/3 re-raises after closing, so a failed write leaves .NAME.saving-* behind and the page's "a failure removes the new file" is false. Expect: :file.write with {:error,_} matched or rescue+rm, plus a test. P2s: read's stat-then-whole-read growth race, unchanged/2's uncapped read, rm of another saver's temp on :eexist.
- B34 (wp-B34 7cf1fdcb7): OK. P2 page note open: FunDecoded fun whose module is later loaded with its checksum compares unequal to that code's funs (BEAM: equal) — a third known difference beamlet.md should state.
- B35 (wp-B35 81497bcf8): OK with notes. P2 open: the four BEAM4 manifests (tests/data/beamlet/{launch,natives,serve,natives-attack}.json) publish beamlet + 3 natives ≈ 1,050 pages on rv32, so bootfsd's cap 2,048 is under twice the peak (2,176 by B32's rule); no scan there, so nothing fails.
- K26 (ac6e2698e): OK with notes; P1 on the claim "slots came back" (4 slots, 3 logins): expect `sshd: connection N on slot [01]` for the third login, or four logins.
- BEAM14 (074922f8e): OK after the fold (sessions overlap now). SRV1 (0bc07b8dc): OK. SHELL8 (8306ef6ed): OK. SHELL10 (b0ea0ff99): OK. B27, BOOT2 (a3e4e6fd3), B31, STEWARD2 (d9a6952e4), K23 (8865a7a05): OK/OK with notes, nothing open from me.

## CTX1 attack notes for CTX2/3 (takeover, cap, detach, restart)
1. Order of guards in session.md: login_key must stay first; anything revealing (InUse, LockedOut, cap refusals) only after it. A cap refusal (rule 6) must also come after login_key and be told only to the right key — and "failed authentication stays cheap": the cap check must not run before the key check.
2. Takeover (rule 5) is the dangerous one: a second login with the right key takes a live context. Check: the console relay hands the VM's console to the NEW channel only after the new login is fully authenticated in the core (not on sshd's word); the old channel is told and ended (status, time, client address — the address comes from sshd: is it trusted text? it must pass the terminal guard); the old sshd slot is freed (K26's Reader/GONE path); no window where both channels are attached; a labelled context never attaches to a channel of another label set (rule 4; sshd's channel labels re-check).
3. Detach/reattach: a detached context's budget and VM stay — the sub-budget cap/R37 flood case (steward-sub-budget-flood) must still hold with detached VMs counting; the bounded output buffer (rule 9) lives in the steward's heap (cap 28 pages!) or the relay's — check what it is charged to and that a flooding detached VM can't grow the steward's heap (R26 "steward work is paid by the steward" residual).
4. Restart rule (8): with K23's reap, contexts end with the steward; a name must never reattach to something that merely reuses the name — ids are MINTED|random; check the relay/sshd side forgets ids on GONE (K26 generation) so a stale channel can't attach to a new context of the same name.
5. Enumeration/timing: rule 3 is content-only today; the pages should say so (my note). CONTEXTS in the model includes non-names; keep P17 when adding cap/takeover ops.
6. Reserved-name list is duplicated: sshd Login::RESERVED and libs/steward manifest::RESERVED — a new reserved name must land in both (sshd doesn't depend on the steward crate).
7. Interim in_use for a second `ssh alice@box` becomes takeover in CTX2: the steward-context-login case's expect_after line for InUse must change then.

## Standing rules learned
- A launch worktree may move under you (rebase/fixup); always `git rev-parse HEAD` and range-diff against the assigned head; report both hashes.
- Verdicts on claims: a case must judge, not sequence away, what its description claims (BEAM14 overlap, K26 slots).
- Memory caps: B32's rule is twice the largest peak rounded up to 128, per manifest; check every manifest copy that shares a server entry.
- Reviewer reports go as message_send answers (reply_to the instruction); there is no assignment id to complete.
