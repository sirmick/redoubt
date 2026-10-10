# K26 report

Branch wp-K26 on main ac178530e, two commits, never pushed:

- 7be024a27 sshd: a slot comes back whichever side ends its connection first
- ac6e2698e sshd, steward: a steward's end ends its sessions' channels

## Design (approved at the checkpoint)

The kernel tells sshd that the steward has ended:

- sshd keeps one `watch` call parked at the steward. This is a new steward message, opcode 13,
  accepted on sshd's root badge only. The steward holds it unanswered while it runs (at most two,
  dropped on the kernel's abandon notice).
- Whichever way the steward ends, the call ends with it: Dead (R4b) when it dies, or an answer when
  it drops the call on a clean exit. That also covers a steward that never comes back.
- sshd's watcher thread then moves a generation on (an AtomicU32, since rv32 has no 64-bit
  atomics) and sends GONE(generation) to every slot.
- A slot whose session predates that generation ends the channel with status 1 and says
  "sshd: session <id> ended: the steward is gone". Its slot returns.

## The orchestrator's four conditions

1. **The slot leak is its own commit, with a host test.** Done (7be024a27).
   - Cause: after `drive`, `connection()` waited for the reader's EOF, which `drive` had already
     taken when the client hung up first. The slot never returned; slots went 0..3 across
     ordinary sessions.
   - Fix: `slot::Reader` (servers/sshd/src/slot.rs) records whether the EOF was taken.
   - Host tests in servers/sshd/tests/slot.rs: a dozen connections, ended by the client or by the
     server, each take slot 0 again.
2. **The watcher never spins.** It calls again at once after a call the steward held, and pauses
   1 s after a call that ended at once. It ends with the machine if init reboots.
3. **A login in the gap.** A login, or a channel's close, waits at most 30 s for the steward
   (`redoubt_client::typed::call_within`, new) and is then refused, so it never hangs the slot.
   sshd.md, "The steward's end", states the bound.
4. **The case proves the slots come back.** steward-restart-ssh:
   - alice's shell is busy, so closing her input cannot end her session. Her ssh exits 1 when the
     steward goes.
   - bob logs in to the restarted steward and exits 0.
   - alice's vault then logs in.
   - It forbids "every slot is busy".

Host test: `watch_is_held_only_from_sshd_and_malformed_on_any_other_badge`. `protocol::watches`
is true only on SSHD, and `watch` on every other badge answers Malformed.

## Also changed

- **Restart-probe:** it now serves until its 14 s deadline (a receive timeout), so a probed
  steward takes logins and watches. Before, it slept before its loop and served nothing.
- **Residuals:** sshd.md and steward.md lose theirs: the channel outliving its session, and the
  slot.

## Gates (through q, logs in /var/tmp/redoubt/K23/K26)

- Host tests:
  - `cargo test -p redoubt-sshd -p redoubt-steward-server -p redoubt-wire`: 0.
  - `q run --quiet -- cargo test -p redoubt-client`: 0.
- prebuilt: 0.
- Each of these exited 0 on rv64 and rv32: steward-restart-ssh, steward-restart (13 restarts),
  steward-restart-reboot, steward-ssh-two-principals, init-boot.
- Also exited 0, on rv64 and rv32 in the first run, with only the rv32 atomic changed since:
  ipc-outcomes, bench-net-peer.
- unsafe-budget, no-cruft, docs, formatting: 0. size-budget: 0, after these raises:

  | Crate | Old | New | Why |
  | --- | --- | --- | --- |
  | libs/client | 1,078 | 1,088 | call_within |
  | libs/wire | 3,657 | 3,675 | generated watch |
  | servers/sshd | 1,131 | 1,200 | the watcher, GONE and the bounds, with commit 1's slot reader |
  | servers/steward | 1,193 | 1,209 | watch held, probe serving |

  Each has a `Size budget:` line in commit 2.
- **userland-boot fails on rv64 and rv32, in both runs.** It times out waiting for
  `/ \(N\)> 55$/`. The shell does print 55, but on its own line, not after the echoed prompt:
  that is the shell's terminal rendering. The K26 diff touches only sshd, the steward, the wire
  table and the typed client; nothing in the shell, beamlet or the console path. I did not run
  main ac178530e to confirm it fails there too.

## Pages

- sshd.md: "The steward's end" bullet (`watch`, GONE, status 1, the 30 s bound); the slot rule in
  "The box's platform"; status lines (the slot host tests, bench:steward-restart-ssh).
- steward.md: "`sshd` learns of the steward's end from the kernel"; the probe now serves; status
  lines (steward-restart-ssh and the watch host test).
- libs/wire/tables/steward.md: row 13 and the badge-class sentence.
