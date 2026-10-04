# INIT3: init restarts a server, reboots when one cannot stay up, and releases a child's grants

Tier A (`init` is the trusted base), size M, needs INIT2. Start from main once INIT2 has merged.
Run every cargo and bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from
the worktree.

## Context rules (read these first; context ran out four times on INIT2)

- **Don't read whole files.** Run `grep -n` first, then Read a range. `servers/init/src/bin/init.rs`
  is 550 lines and `check.rs` is 614; you need only the parts named below.
- **Don't open `.wash/qa/*.md`, INIT2's reports or other briefs.** Everything they decided that
  you need is in this brief. If you must open a QA file, read it only up to its checkpoint
  comment: `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Use `cargo testbench --list | awk '{print $1}'`. Read boot logs under
  `target/testbench` only through `grep` or `tail`, because they start with hex dumps.
- **Read a file right before you Write it,** and prefer Edit.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/INIT3-report.md`.
- **If you hand off, keep the handoff short** and end it with a section "what consumed my
  context". Your successor reads that handoff and this brief, not the reading list again.

## Reading list (only these, only these ranges)

- `docs/servers/init.md`, these sections: "Restarts and reboots", "Fresh connections per child",
  "Authority", "Failure and restart" and "Residual risks".
- `docs/servers/wire.md`: "A launcher releases its child's grants", about 20 lines.
- `docs/servers/serving.md`, "Replies and rollback": only its status line and its first two
  paragraphs.
- `docs/kernel/devices.md`: the bullets on `init`'s copies and the quarantine reboot, under
  "Which process gets which device" (find them with `grep -n "kept copy\|reboots the machine"`),
  and the status line of `system_reset`.
- `docs/testbench.md`: "The servers' cases under `init`".
- `servers/init/src/bin/init.rs`, from `fn run` to the end of `fn watch`, then `struct Watched`
  and `extern "C" fn watch`.
- `libs/client/src/launch.rs`: `Launch::grants`, `Job` and `Job::wait`, which already release a
  child's `Grants` on its exit notice. Also `libs/client/src/grants.rs`, but only its `pub fn`s.
- `libs/sys/src/record.rs`: `struct ExitNotice`. It already carries `blamed_account` and
  `blamed_labels` (R21).

## What INIT2 left, and who builds it

| Left | Whose |
| --- | --- |
| Restarts, the reboot rule, fault reports, exits during the boot | this package |
| A launcher releasing its child's grants (wire.md), with its attack case | this package |
| A case that watches a child's console connection disconnected at its exit | this package |
| Rollback, and the malformed fallback, in a boot | this package |
| Driver restarts and the quarantine reboot | this package, with a test driver |
| `netd`'s restart case (netd.md "Started by `init`") | **INIT4**, with the net cases under `init` |
| Step 6: the steward and `sshd`; `blame`, its badge, a wedged steward, the steward's own restart | the steward step |
| An `fsd` for each volume | the fsd step |

**Blame waits for the steward.** The owner approved this with `init`'s design. So no stand-in
steward and no `blame` call: init.md already says "A fault before the steward runs is reported on
the console and blamed on nobody", and until the steward exists every fault is one.

## The rules (the page states most of them; the new ones are marked **new**, with their lines below)

1. **A restart** happens when a server exits, faults or is killed. `init`:
   - disconnects the dead instance's console connection and releases any other grant it made
     for it;
   - destroys the dead instance's budget;
   - carves a new budget from the manifest entry;
   - mints the `handed` badges again from the receive rights it keeps;
   - makes a new console connection;
   - starts the new instance through the stub, with the same name, arguments and receive
     endpoint and a new startup block.

   A driver's device handles are placed again from `init`'s copies. `init` never receives on a
   server's endpoint.
2. **Release a child's grants through `Launch::grants` and `Grants`.** `init` no longer
   disconnects the console connection by hand, and it never calls `Job::wait`, because its
   watching threads take the notices. Releasing through `Grants` is wire.md's rule for every
   launcher. If `Grants` does not fit `init`'s threads, say why in the report and keep the
   direct disconnect.
3. **New: the boot's own steps, again.** A restart repeats what the boot did after starting
   that server:
   - `keyd`'s key-separation check;
   - attaching `init`'s own connection to `consoled`;
   - the `public` entries pushed to `bootfsd`, and the seal.

   If one of these fails, `init` reboots.
4. **New: during the boot too.** The watching starts with the first server. If a server exits
   before the boot is done, it is restarted and counted the same way, and a step that was
   calling it waits for the new instance and tries again. So a `consoled` refusing a `buckets=N`
   its budget cannot hold reboots the machine by rule 5.
5. **The reboot rule.**
   - On a server's exit, if it was already restarted 5 times within the last 60 seconds of
     `time_now`, `init` prints its reboot line and calls `system_reset(Reset, 2)` instead of
     restarting it.
   - Keep the last 5 restart times for each server.
   - Host-test the boundary: the fifth restart goes ahead, the sixth exit reboots, and a restart
     older than 60 s is dropped from the count.
6. **New: a quarantined driver.** When a driver exits, `init` asks `device_info` of each device
   handle it keeps for that driver. If any is `BadHandle`, the device's quarantine destroyed its
   object, and `init` reboots instead of restarting the driver.
7. **A fault's report.**
   - `init: NAME (PID p) faulted, code c, serving account A, labels L; blamed on nobody: no
     steward`. Use the notice's `blamed_account` and `blamed_labels`, and print "serving nobody"
     when the account is 0.
   - Then `init: restarted NAME, console N`.
   - An exit or a kill keeps INIT2's line, followed by the same restarted line.
8. **New: a restarted `consoled`.**
   - `init` attaches again and prints `init: restarted consoled; every other server's console
     connection is gone until it restarts`.
   - No server writes to the console today, so nothing is lost yet.
   - If `consoled` cannot start at all, `init` has already given up the UART. Its lines about
     the restarts and the reboot then go nowhere: residual 2 below.

## Page lines (exact; write each one in the commit that makes it true)

**init.md, "Restarts and reboots".**
- Status becomes "built · partly tested: blame, `blame`'s badge, a wedged steward and the
  steward's restart are the steward's, not built; a restarted `consoled`'s attach is read from
  the code, not attacked · tested: …" with your cases and host tests.
- In "**A driver**", replace "A driver whose device was quarantined is not restarted: `init`
  reboots" with: "A driver whose device was quarantined is not restarted: `init` finds its own
  copy of the device handle closed, and reboots".
- After that bullet, add:
  > - **The boot's own steps, again.** A restart repeats what the boot did after starting that
  >   server: `keyd` is checked again ([the key-separation check](#the-key-separation-check)),
  >   `init` attaches to a new `consoled` again, and it pushes the `public` entries to a new
  >   `bootfsd` and seals it again. Until then a client meets the new instance as the boot left
  >   it: `bootfsd` answers "does not exist", never a half-written entry. If one of these steps
  >   fails, `init` reboots.
  > - **During the boot too.** A server that exits before the boot is done is restarted and
  >   counted the same way, and a step that was calling it waits for the new instance and tries
  >   again. So a server that cannot start, `consoled` refusing a `buckets=N` its budget cannot
  >   hold among them, reboots the machine by the rule below.
- The figure's caption becomes:
  > *Figure: a system server's restarts. Until the steward runs, a fault is reported on the
  > console and blamed on nobody.*

**init.md, "Residual risks".** Add:
> - **A restarted `consoled` forgets every server's console.** The servers' connections lived
>   in the dead instance's tables, and `init` has no way to hand a running server a new one, so
>   a server's lines are refused until it restarts too. `init` attaches again and says so.
> - **A console that cannot start reboots silently.** `init` has given up the UART, so its
>   lines about `consoled`'s restarts and the reboot go nowhere.

**init.md: the other status lines.**
- "Fresh connections per child" and "Authority": drop each partly-tested clause your cases close.
- In "Authority", "restarts are not built" goes.
- In "Failure and restart", the pointer stays.

**wire.md**, "A launcher releases its child's grants": planned becomes built, with the case.
Change the text only where the code differs, and tell me where.

**serving.md**, "Replies and rollback": add your cases to the status line, with:
> partly tested: the exit when even the malformed reply is rejected is host-tested only, since no
> caller can make the kernel reject it

**devices.md.**
- "Which process gets which device": the clause "restarts, which place a kept copy again, and
  the quarantine reboot are not built" goes.
- `system_reset`: the clause "a reboot (`kind` 2) is not attacked by a case; …" goes, and the
  reboot cases join its list.

**testbench.md**, "The servers' cases under `init`": after the sentence ending "a second such
line fails it", add:
> A restarted server is announced as `init: restarted NAME, console N`, and the bench never
> reads a reporter's id from that line, so a reporter that restarts cannot pass its case. A case
> that judges a reboot expects `init`'s reboot line and then the next boot's first line, and
> ends there.

Use the real first line of a boot; tell me which line it is.

**SECURITY.md**: its rows follow any status that changes.

## The cases (both widths, in a checked build; names are suggestions)

New test programs go in `tests/init-programs` (servers' cases) or `tests/programs` (the tester
case). Every verdict comes from a trusted line (testbench.md, rule F): `init`'s own bare lines,
the kernel's, or the reporter's.

1. **`init-restart`.** A test server built on the serving library takes a labelled client's
   connection and calls. The server holds a copy of its own console connection, which it hands
   the client at the start. On one request it faults while serving the client.
   - `init` prints the fault line with the client's account and labels, then the restarted line.
   - The client's call gets `Dead`. Its retry is served by the new instance, and its old
     connection id is refused there.
   - The dead instance's console connection, written through the client's copy, is refused:
     this is the "Fresh connections per child" item.
   - On another request, before the fault, the server answers with a reply the kernel rejects
     (a handle it does not hold). The client gets the malformed reply, and the server goes on
     serving.
   - Under `init` an exit is a restart, so every test program parks when it is done and never
     exits, except where an exit is the point. The restarted server must not fault again.
2. **`init-rollback`.** A client whose handle table is full asks `consoled` for
   `new_connection` more times than its bucket holds. With one slot free, a connection still
   stands. This is `ninep-newconn-discard` against a real shared server. It may share a boot
   with case 1.
3. **`init-reboot`.** A test server that exits at its start, listed before the last server so
   that it exits during the boot.
   - Verdict: its exit and restarted lines five times, then the reboot line, then the next
     boot's first line.
   - Host tests in `servers/init` cover the counter's boundary.
4. **`init-driver-restart`.** A test driver in a case manifest is given an empty virtio-mmio
   slot with DMA allowed. It `dma_alloc`s and then faults.
   - `init` restarts it, and the new instance finds its device by name and `dma_alloc`s again.
5. **`init-quarantine-reboot`.** The same driver on a kernel built with `dma-reset-deaf`.
   - Verdict: the kernel's quarantine line, then `init`'s reboot line naming the device, then
     the next boot's first line.
6. **`launcher-orphan`** (wire.md's attack test). This runs under the tester in `init`'s place,
   because servers under `init` hold no budget handle (R33).
   - The tester starts L with `Launch` and a connection at `ninep-discard-server`, recorded in
     L's `Grants`.
   - L starts C with a connection minted through L's own connection. C runs in a budget the
     tester carved as L's sibling and gave to L, so L's end does not kill C.
   - L faults. The tester's `Job::wait` releases L's grants.
   - The server's verdict: L's connection and C's, minted under it, are both gone. C's next
     call through its connection is refused.

## Owned paths

- `servers/init/**`.
- `libs/client/src/launch.rs` and `libs/client/src/grants.rs`, only if rule 2 needs it.
- New programs in `tests/init-programs` and `tests/programs`, and new `tests/*.toml` cases.
- The page sections named above.

**Not yours; ask first:**
- `libs/rt/src` and `libs/rt/tests` belong to RT1, which follows INIT2.
- `libs/rt/fake`, `libs/sys`, the kernel, budgets.md and processes.md belong to K16.
- devices.md's `dma_alloc` section belongs to K21.
- `tools/testbench` belongs to B7. Rule 7's lines and testbench.md's sentence should need no
  bench change; if one does, ask.

`netd`'s restart case moved to INIT4: don't build it here.

## Gates

- The whole bench on both widths, alone (one whole bench at a time).
- `servers/init`'s host tests.
- `cargo fmt --check`.
- The size budget: `init` grows; give the number.
- The unsafe ratchet: `init` has none and keeps none.
- doccheck.

Report each command with its exit code. The report lists:
- each rule, with the code and the case or host test that shows it;
- what was deleted (the hand-written disconnect, if rule 2 lands);
- each page line, as written.

## Checkpoint

After rules 1 to 5 and case 3 pass on one width, send one progress line with the branch and the
case's lines. Then go on.
