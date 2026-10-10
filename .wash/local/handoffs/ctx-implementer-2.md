# Handoff: ctx-implementer-2 (CTX2 phases 3-6 mostly done, WIP; checkpoint)

## Branch state (nothing pushed)
- Worktree /home/mcloonan/redoubt/.worktrees/CTX2, branch wp-CTX2 on origin/main 6a684f2cb (CTX1 merged). Tree clean. Head 53d4462cf. Six WIP commits:
  677560152 core/model/reference/wire (phase 1+2, predecessor), 83500fc37 consrelay crate, c3a97e69e steward machine relay steps + sshd two consoles + login wire, 0670e04f8 pages/cases/budgets + input-EOF detach, ea37547be typed input outlives detach + steward sizes + consrelay in bundles, 53d4462cf model counts + restart-ssh/session-ends rewrites.
- A bench run may still be going: .tmp/p6d.log (prebuilt, rv64 restart-ssh + session-ends, then the rv32 set). Seen so far: all rc=0 except rv64/steward-session-ends (flake, see trap 1). Still pending when I stopped: rv32 session-ends, ssh-two-principals, vault-launch, sub-budget-flood.
- rv64 full steward-*/sshd-*/consrelay-* set: all green except session-ends (trap 1). Host: consrelay 16 tests, steward-server 20, sshd console 7, model host + steward-model-host-tests green (LAST/counts re-measured). docs rc 0, unsafe-budget rc 0. size-budget fails ONLY for missing `Size budget:` lines (see Left).

## Rulings in force
- H changed to (c) (orchestrator): sshd mints TWO rooted console connections per login, sends both (`login` gains `relay: handle[1] endpoint`); steward keeps the first (its `ended` ends the channel), hands the second to the relay in `attach`; sshd honours `ended` only on the steward's badge (`console::Consoles::may_end`). LIMITS stays buckets 2, state 2 (two mints/login, one login/channel). Recorded in .wash/local/CTX2-design.md Decisions. Orchestrator conditions: wire + sshd.md + steward.md in the same commit (fold when cleaning); sshd host test (done: a_login_s_two_consoles_and_only_the_steward_s_ends_the_channel; VM holds no sshd badge now, covered by "any other badge"); machine step where the VM sends `ended` and the session stays (done in steward-context-login, passes both widths).
- PIPE1 (pipe-implementer, orchestrator's ruling): sessions get processes 10 (VM, relay, piped, 7 stages), alice 40, bob 20, in image/manifest.json and the copies in boot-profile, steward-vault-launch and beamlet-footprint. I set session processes 3, alice 12, bob 6 (and case lines "6 processes"/"3 processes"). Whoever lands second rebases onto the other's numbers; with PIPE1's 10 the case lines `holds N pages, 3/6 processes` change. I did NOT update beamlet-footprint's manifest copy: check it.

## What is built
- servers/consrelay (lib Relay pure state + bin 3 threads; hello on startup handle "hello"; control attach/detach on badge 1; reader 2, writer 3; VM console minted rooted; labels learned from its own threads' kernel-stamped first call; KEEP 64 KiB, cut-to-line + drop line on attach; detach waits for its note; input kept across detach, cleared on takeover).
- Steward machine (servers/steward/src/bin/steward.rs): hello endpoint, launch_relay (unstamped hello badge: the kernel refuses a stamp outside the endpoint's own), waits 2 s for hello, VM badge used for the next console slot; attach/detach bounded 1 s; Abandoned watch => EventKind::SshdGone. Steward stack 8/heap 30 in all 3 manifests; docs/testbench.md row updated.
- sshd: client input EOF (after the session read it all) => channel_closed to the steward + channel ends status 0 (Slot::closed, once). This is how "closing SSH detaches" works.
- Pages: steward.md (Contexts rewritten, R80 section, guard row, failure, login two connections), SECURITY.md R80 row, consrelay.md (new, SUMMARY), sshd.md, sessions.md, model.md (counts 2,000/3,000, LAST (114,128),(675,131), catch floor 1,418/2,693, R80 mutation row, 158 variants, refuse_in_use never reached residual), plan/m2.
- Cases: steward-context-login (takeover both terminals + VM `ended` step), steward-session-ends (detach/reattach/exit), steward-restart-ssh (bob's ssh delayed 14 s by ProxyCommand `sh -c 'sleep 14; exec nc %h %p'` so alice stays attached across the restart), count lines, consrelay added to programs lists of boot-profile(-unverified), image-disk, userland-read-only, steward-restart(-ssh,-reboot); new cases consrelay-build, consrelay-host-tests.

## Left
1. Trap 1 fix (not yet done): relay drops the last typed bytes when the steward's detach reaches the relay before the reader's INPUT call (race seen once on rv64 session-ends: `exit` lost, alice stays 6 processes, case times out). Fix in servers/consrelay/src/lib.rs: remember the generation just detached (`detached: Option<u64>`, set in detach, cleared in attach) and let give_input accept that generation while no newer channel is attached; host test; rerun session-ends both widths a few times.
2. Remaining design-note cases not written: steward-context-labels (vault + unlabelled same name, relay label attack), sshd-restart-detaches machine case, buffer-bound machine case. Ask orchestrator which are required (host tests cover buffer and SshdGone).
3. Phase 5: measure the relay's pages (B32: twice peak, rounded to 128) and size sessions; currently relay stack 8 / heap cap 48 pages are guesses; testbench.md says "not yet measured". Coordinate with PIPE1's sizes.
4. Gates still to run: elixir-oracles (wire elixir regenerated for login's relay handle; the Elixir steward reference may need it), userland-boot + beamlet set both widths, footprint, rv32 rest.
5. Commit cleanup: fold the WIPs into logical commits (testbench name test, relay crate, steward/sshd/wire+pages, cases...), with Size budget lines: libs/wire, libs/steward, model, servers/sshd, servers/steward, servers/consrelay (new crate). size-budget passes only once those lines exist.

## Traps
- NEVER edit the worktree (docs included) while `make prebuilt` or cases run: they fail "tree changed" (lost two runs). Commit, prebuild, run, then edit.
- The net lock queue can be slow (other tenants); jobs wait 20+ min.
- rustfmt +nightly per file with --edition 2021; scripts/q for every cargo.
- `stale close` answers Unknown (the id is forgotten), not ok; pages say "names nothing and changes nothing".
- steward's `ended` send after channel_closed may stall the steward up to RELEASE_TIMEOUT (1 s) since sshd's slot is in the call; sshd ends the channel itself, so harmless but costly — note as residual if a reviewer asks.
