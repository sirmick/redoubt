# B30 report

Branch wp-B30 on main 3b049cacc, worktree .worktrees/B30. Two commits:

1. `025ba6a33` testbench: a console line is matched as a terminal shows its text
   - `tools/testbench/src/qemu.rs`: `shown(line)` strips a CSI sequence whole, any other escape
     with the char after it, every other ASCII control but tab (DEL included), and trailing space.
     Nothing is rendered, so a CR cannot move text in front of the line's prefix.
     `Console::next` matches expect, expect_after, input/poke `after` and distinct_across_boots
     captures on the shown line, which `Line::Text` carries. `forbidden()` matches forbid and the
     always-forbidden on raw OR shown. The PASSED/DONE forgery checks fire on either form; the
     reporter's prefix, the DONE regex, the loader line and init's announcement are still judged
     on the raw line. The log keeps raw lines.
   - Host tests: `a_line_is_shown_without_its_control_sequences`;
     `a_shown_line_keeps_its_own_prefix_first` (`[con A] \r\e[J[con B] x` shows as
     `[con A] [con B] x`, and neither `^\[con B\] x$` nor `^\[con [0-9a-f]{16}\] x$` matches);
     `forbid_matches_the_line_as_it_came_or_as_shown` (`PA\e[0mNIC` caught by PANIC; a raw ESC
     caught by `\x1b`).
   - docs/testbench.md "What a case passes on": the rule in one paragraph; status 6 to 9 tests.
2. `dd161bdad` tests: the shell's input line and its result, each on its own console line
   - userland-boot: `/ (N)> Enum.sum(1..10)` then `55`; `/ (N)> 1 + 1` then `2`.
   - userland-read-only: the entered Version line, then `"1.2.3"`.
   - boot-profile and boot-profile-unverified: `first console read` and `boot-stats: loads`
     before the banner, then the entered line, then `55`. Comments updated.
   - docs/userland/beamlet.md (beamlet on Redoubt): the stamp is now the driver's first read
     (`console_subscribe` in driver.ex, before the banner and prompt). The boot to the prompt is
     the stamp plus the shell's own start, which the stamp no longer counts. The table's rows were
     taken under the old stamp. **Design point:** the boot-time target ("the prompt within 20 s")
     now measures less than the prompt. A follow-up should move the stamp to the first drawn
     prompt, or remeasure.

## Gates (exact commands, from .worktrees/B30, env exported)

- `q run --cores 4 -- cargo test -q -p testbench`: 154 passed, exit 0 (final tree).
- `make -f scripts/jobs.mk prebuilt`: rv64 230 and rv32 216 cases, 0 failed.
- `make -k -f scripts/jobs.mk rv64/<c> rv32/<c>` over the 9 cases: 14 PASS, 4 FAIL.
  PASS on both widths: userland-boot, userland-read-only, steward-vault-session,
  steward-session-ends, steward-ssh-two-principals, boot-profile, boot-profile-unverified.
  - **steward-sub-budget-flood** FAIL rv64+rv32: `session bob: step 5: ssh exited (status 0)
    while waiting for /^55\r?$/`. The same failure, rerun alone (`q run --quiet`), with main's
    qemu.rs: not B30's. Bob's own VM ends while alice's vault session floods: the kernel prints
    `terminate_process: 374`, the steward reports `users/bob/{} holds 0 pages`, and bob's ssh log
    ends with `ok`. Log: target/testbench/run-1819098-1791449781047480384/.
  - **beamlet-footprint** FAIL rv64+rv32: `beamlet: heap needs 11804 (rv32 11406) pages for twice
    its 5902 (5703)-page peak, capped at 10989`. With main's qemu.rs it times out on
    `^\[con …\] 2$` (the '55' failure class). B30 fixes that match, and the memory check behind
    it now fails. The shell VM's heap peak went up with SHELL2's driver. That needs a budget/cap
    decision or a smaller driver: out of B30's scope.
- docs: PASS (`make -f scripts/jobs.mk docs`). formatting: PASS. no-cruft: PASS.
  Run before the comment-only fold; docs was rerun after it.

## Docs check

Changed: docs/testbench.md, docs/userland/beamlet.md. Checked, no change needed:
- docs/testbench.md "Checked builds" boot-profile passage: it says the boot's time to its prompt,
  which is still the target's wording; the design point is above.
- README.md, GETTING-STARTED.md, userland/shell docs: no claim about the console's '(N)> 55'
  lines (grep for '> 55', 'follows the prompt').
- tools/testbench/src/ssh.rs: SSH sessions keep their own rule (CR dropped, multi-line, patterns
  unanchored). Not changed, since no session needed it.

## Open

- The two failures above need direction: B30 can merge with them as known main failures, or
  wait for their fixes.
