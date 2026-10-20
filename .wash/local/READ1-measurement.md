# READ1: the measurement (phase 1)

Branch `wp-READ1`, case `tests/lease-code-load.toml` (WIP commit), worktree
`.worktrees/READ1`; logs under `.worktrees/READ1/.tmp/READ1/` (`runN-<width>-smpM/` holds each
boot's console and session logs); `analyse.py` and the per-run summaries are the orchestrator's to
re-derive from those logs.

## The short version

The lease VM's death STEWARD9 saw is **not a drain-rate or timeout matter**. A lease VM's code
reads are synchronous client calls with `FOREVER` and no `busy` handling, so `FILE_WAIT_US`
never applies to them; nothing times out. What kills (or parks) the VM is **erofsd's admission
answering `too many open files`**: the per-badge fair share of `Files` (R26,
`libs/rt/src/server/admit.rs`: a share may take one more while it holds fewer than
`max(1, limit / (holders + 1))`) with `limit = 32` (`servers/erofsd/src/server.rs::limits`), where
**every connection of the account holds one fid for its life, its root fid charged at `Tattach`**
(`libs/rt/src/server/ninep.rs`, `Tattach` → `reserve_fid` → `admit(Files)`). So with `n`
connections of one account at the system volume the share is `max(1, 32 / (n + 1))` fids each,
root included: at `n ≤ 15` a connection may walk one fid beside its root; at **`n = 16` the share
is the root fid alone and no connection of the account can walk, open or read anything** — every
module load of every VM of the account is refused at once. The same arithmetic holds at
`walfsd:data` (`servers/walfsd/src/server.rs::limits`, `files: 32` as well), the leases' `/project`, so an agent
cannot read its input or write its result either.

Two session connections (the console session's and the SSH session's shell VMs) are always
among the `n`, so the ceiling is **13 live leases of one account**; the 14th makes `n = 16`.
STEWARD9's N = 16 round ran fifteen spinners beside the two sessions: `n = 17`.

What a refused read does to a VM, by where it lands (`userland/otp/redoubt/src/bin/beamlet.rs`,
`userland/otp/redoubt/src/lib.rs`):
- the platform's own attach of the lease's `/project` (its root fid at walfsd) refused → `Redoubt::new`
  fails → **exit 14** under `exit-kinds`, 1 in a default build, with no line (**seen**: rv32 at two harts, agents 13 and 14,
  `exited 14`, the data volume's bucket of 32 full of twelve roots and the sessions' fids);
- the boot pack's read or the start module's first read refused → the VM **parks**
  (`sleep(FOREVER)` for good): alive, idle, no exit, no line (a lease has no console);
- the start module's second read, inside `vm.spawn`, refused → `beamlet: … did not start` →
  **exit 1** (this is STEWARD9's `agent-36 … exited 1`: launched at 460.4 s, it was still in its
  start-up reads 107 s later when the fifteenth spinner connected);
- a module loaded lazily while the agent runs refused → `Lookup::Refused` → `undef` in the
  process → an exception out of the start function → **exit 0** with an `{'EXCEPTION',…}` line
  nobody sees;
- the agent's own `File.read("/project/in")` refused (the hub maps `too many open files` to
  `emfile`) → `Redoubt.Agents.Echo` writes the error as its result (refused too) and **spins**
  until its lease ends.

So the four levers of the node body read against this:
- *the pages/requests VM-count term*: not on this path at all (`Requests`/`Pages` are the
  multiplexed hub's; code reads are plain calls);
- *verityd's cache and read-ahead*: a rate matter, measured below, never the cause of a death;
- *the read path's weight*: the same;
- *the VM's resilience to a slow read*: the read is not slow, it is refused; the question is
  whether a refused code read may be retried or must be fatal, and separately why a lease's
  share is one fid.

The lever that matters is a fifth: **the `Files` cap (and its share rule) against the account's
connection count** — the derivation that gave `state: 28` (two per session and one per lease,
held by the bench against every manifest: `tools/testbench/src/case.rs`
`every_manifest_entry_holds_its_buckets_at_their_caps`) has no counterpart for `files: 32`,
whose share must leave each of `n` connections its root fid *and* one working fid: `files ≥
2 (n + 1)` under the current share rule, or a share rule that does not count the root fid.

## How it was found

(the runs, in order; each boot rv64, one hart, seed 3, under q)

1. Sixteen launches back to back, all served in 2.4–3.2 s; then the SSH session's own module
   reads failed (`Elixir.Process not loaded: its file could not be read: failed`), its wait step
   crashed, the bench ended the leases. `failed` was `unread`'s catch-all arm.
2. `unread` taught to name the error: `refused` — an `Rerror` outside not_found/corrupt/other.
3. `icount` with `sleep` on (a disk case's rule) — no change: the stall is not the clock.
4. The agents wrote no file at all, not even Echo's `result`; the guest ran at full speed
   through a 600 s wait (620 guest-s in 110 wall-s): sixteen VMs spinning, verityd's counters
   frozen at its 2048th read.
5. The shell's probes named it: `Elixir.Base not loaded: its file could not be read: too_many`
   in 72 ms, and erofsd's `boot-stats` sentinel walk refused the same way — erofsd alive and
   answering, the account's shares spent.
6. Twelve agents, then a thirteenth and a fourteenth: at 13 the shell still loaded `Base`
   (4.1 s) and erofsd answered its sentinel; at 14 the sentinel walk was refused `too_many` —
   the computed edge (`n = 16` holders).
7. Steps of four with a read under `/project` probed too: `enoent` at 4, **`emfile` from 8**
   (walfsd's share of the same rule), the shell's own `end_lease` comprehension refused the
   modules it needed at 14 (`elixir_map`), so the leases were not ended and nothing could be
   listed.
8. The listing moved after the leases end (the shares come back), the shell's later needs
   warmed first, waits of 240 s: the runs tabled below.

## The measurement

The case (`tests/lease-code-load.toml`, final shape): alice's SSH session warms the modules its
own later steps need, then starts agents in steps (1–4, 5–8, 9–12, 13, 14), each a thirty-minute
lease running `Redoubt.Agents.Echo` with the input `load`: the agent writes `born T` to
`/project/out/load-T` before anything, loads 27 modules from the system volume (2.2 MB before
stripping) timing each on the kernel's clock, writes the lines, and spins. After each step the
session waits up to 240 s of guest time for every agent so far to have written `done`, then
probes its own view: a module load of its own at the system volume, a read of a missing file
under `/home/alice/project` (`enoent` while walfsd admits it, `emfile` when not), and a walk of
erofsd's `boot-stats` sentinel (erofsd prints its 9P counts if the walk is admitted). At the end
the session ends every lease (which gives it its shares back) and prints every agent's file.
beamlet is built with `exit-kinds` so a VM's exit code names its end (11 did not start, 12
stuck, 13 halt, 14 no platform); erofsd and verityd with `boot-stats`. Every boot passes: it is
a measurement, its output is what is read.

Launch times are the steward's `served start_agent`; an agent's start-up is its `born` less its
`AgentStarted` (both on the kernel's clock; agents paired with starts in birth order within a
step, so the pairing is approximate by a few seconds).

### rv64, one hart (`run8-rv64-smp1/`, seed 3)

| step | leases live | shell's load of its own | shell's read under `/project` | erofsd's sentinel | agents of the step with a file / done |
| --- | --- | --- | --- | --- | --- |
| 1–4 | 4 | `Base` 1.38 s, served | `enoent` (admitted) | answered: 2,025 9P requests, 15.2 MB read | 3 of 4 (the 4th: no file at all) |
| 5–8 | 8 | `Macro` 26 ms (held already) | **`emfile`** | answered | 2 of 4 |
| 9–12 | 12 | `DateTime` 0.86 s, served | `emfile` | answered: 4,065 requests, 33.3 MB | 1 of 4 |
| 13 | 13 | `Stream` 0.95 s, served | `emfile` | answered | 1 of 1 |
| 14 | 14 | `Regex` 0.45 s, served (run 7: **refused `too_many`**) | `emfile` | **refused `too_many`** (run 7 and run 6 alike) | 0 of 1: no file, no exit line — parked |

- **Launches** (`served start_agent`, µs): 1.28, 1.80, 1.86, 1.93 s (1–4); 2.13, 2.45, 2.90,
  2.98 s (5–8); 2.92, 3.26, 3.81, 3.77 s (9–12); 3.72 s (13); 3.99 s (14). Run 7 (120 s waits,
  the same steps): 1.78 → 2.09 s, 2.28 → 2.98 s, 4.34 → 3.72 s, 5.35 s, 6.54 s.
- **Start-up** (born − started; the boot pack of ≈ 3.8 MB and the start module): step 1: ≈ 35,
  44, 56 s for the three that got there; step 2: ≈ 80, 102 s; step 3: ≈ 91 s (one of four);
  step 4 (alone beside twelve spinners): 50 s.
- **The 27 loads**, step 1's first agent (beside three starting): `Base` (215 KB) 7.14 s,
  `erl_lint` (253 KB) 3.04 s, `Code.Formatter` 2.99 s, `Protocol` 1.69 s, `Module` 1.43 s,
  `Stream` 1.45 s, … `Tuple` (6 KB) 0.20 s; 24 loaded (3 held already), 30.6 s in all, 186 s
  from born to done. Across the seven agents that finished: per module min 0.19–0.32 s, median
  0.83–1.97 s, max 5.0–7.6 s; 30–56 s for the 24; **30–85 KB/s per agent** while it loads; erofsd as a whole
  read 10.6 MB in step 1's 326 s (4.6 MB at the prompt, 15.2 MB at the step's probe), ≈ 33 KB/s
  averaged over start-ups and loads, 33 MB by 1,076 s.
- **The writes of the 2 KB result file** to `/project` (walfsd, through the hub): the last one
  12.8–28.7 s each, two of seven **`etimedout`** (the hub's `FILE_WAIT_US`, 10 s, per request,
  retries counted); born-to-done is 98–186 s against 30–56 s of loads, the difference the
  agent's three writes. The hub path to the data volume is the one that does time out, from
  four agents on.
- **verityd** at its 4,096th read: 11,697 blocks asked, 1,195 held (10 %), 10,502 checked, 4,750
  blkd reads (read-ahead gives ≈ 2.2 blocks a blkd read).
- No agent exited; the agents with no file never got a first write through (their
  `File.read("/project/in")` or first `File.write` refused `emfile` or timed out `etimedout`, and
  Echo then spins by design). The 14th parked: its pack read refused at erofsd (`n = 16`).

The two arithmetic lines meet the data: at the data volume the shell's hub keeps its root and
its working-directory fid, so its next fid needs `32/(others+2) ≥ 3`, i.e. at most 8 other
holders: with the console session that is 7 leases, and the 8th lease's step is where `emfile`
begins; at the system volume every VM keeps only its root fid, so `32/(others+2) ≥ 2` holds to
15 holders = 13 leases, and the 14th lease (`n = 16`) refuses every walk of the account (run 6
and run 7 saw it; run 8 found the edge at the moment one holder was not yet attached).

### rv64, two harts (`run9-rv64-smp2/`, seed 3)

| step | leases live | shell's load of its own | shell's read under `/project` | agents of the step with a file / done |
| --- | --- | --- | --- | --- |
| 1–4 | 4 | `Base` 1.81 s, served | **`emfile`** already | 3 files, 2 done (one agent wrote `born` and its first module, then its writes were refused) |
| 5–8 | 8 | `Macro` 53 ms (held) | `emfile` | 2 of 4 |
| 9–12 | 12 | `DateTime` 1.27 s, served | `emfile` | 2 of 4 |
| 13 | 13 | `Stream` 1.40 s, served | `emfile` | 0 of 1 |
| 14 | 14 | `Regex` **refused** (`nofile`) | `emfile` | 0 of 1 |

- **Launches**: 2.11, 2.37, 2.43, 2.58 s (1–4); 2.61, 3.74, 3.92, 4.14 s (5–8); 4.21, 5.50,
  5.78, 5.66 s (9–12); 8.19 s (13); 5.98 s (14) — slower than one hart, as B88 and STEWARD8
  found for a launch under `icount` at two harts.
- **Start-up**: step 1 ≈ 43, 73, 74 s; step 2 ≈ 78, 98 s; step 3 ≈ 116, 143 s.
- **The 24 loads**: 31–71 s per agent (one hart: 30–56); per module min 0.12–0.56 s, max
  5.0–10.7 s (`erl_lint`, 253 KB, 8.4–10.7 s beside eight and twelve). Last write 8.0–23.2 s.
- erofsd answered its sentinel at every step (6 count lines, 37.3 MB read by the end); verityd
  at its 4,096th read: 11,682 blocks asked, 1,112 held, 4,783 blkd reads.
- No agent exited (no `exited N` line); the agents without a file never got a first write
  through.

The count-based edges are the same on two harts as on one; what moves is how early the data
volume's `emfile` begins (step 1 here, step 2 on one hart: it depends on which fids the shell's
hub happens to hold when it probes) and the times.


### rv32, one hart (`run10-rv32-smp1/`, seed 3)

| step | leases live | shell's load of its own | shell's read under `/project` | agents of the step with a file / done |
| --- | --- | --- | --- | --- |
| 1–4 | 4 | `Base` 1.55 s, served | **`emfile`** | 2 of 4 |
| 5–8 | 8 | `Macro` 38 ms (held) | `emfile` | 2 of 4 |
| 9–12 | 12 | `DateTime` 0.94 s, served | `emfile` | 2 of 4 |
| 13 | 13 | `Stream` 1.21 s, served | `emfile` | 1 of 1 |
| 14 | 14 | `Regex` **refused** (`nofile`) | `emfile` | 0 of 1 |

- **Launches**: 1.80, 2.99, 3.20, 3.26 s (1–4); 3.10, 3.92, 4.69, 4.90 s (5–8); 4.56, 5.49,
  6.38, 6.73 s (9–12); 6.11 s (13); 6.74 s (14).
- **Start-up**: step 1 ≈ 77, 75 s; step 2 ≈ 67, 78 s; step 3 ≈ 98, 138 s; step 4 (alone beside
  twelve spinners) 59 s.
- **The 24 loads**: 29–60 s per agent; per module min 0.15–0.39 s, max 4.2–8.6 s (`Base` or
  `erl_lint`). Last write 9.1–17.6 s; born-to-done 109–182 s.
- erofsd answered its sentinel at every step (37.3 MB read by the end); verityd at its 4,096th
  read: 11,689 blocks asked, 1,146 held, 4,745 blkd reads. No agent exited.

Under `icount` a guest second is a count of instructions, and the two widths run the same
program: the rates are within a few tens of per cent of rv64's at one hart, and the count-based
edges are identical.


### rv32, two harts (`run11-rv32-smp2/`, seed 3)

| step | leases live | shell's load of its own | shell's read under `/project` | agents of the step with a file / done |
| --- | --- | --- | --- | --- |
| 1–4 | 4 | `Base` 2.32 s, served | `enoent` | 1 of 4 |
| 5–8 | 8 | `Macro` 68 ms (held) | **`emfile`** | 1 of 4 |
| 9–12 | 12 | `DateTime` 1.80 s, served | `emfile` | 1 file, not done (`born` only) |
| 13 | 13 | `Stream` 1.85 s, served | `emfile` | 0: **`exited 14`** |
| 14 | 14 | `Regex` 0.80 s, served | `emfile` | 0: **`exited 14`** |

- **Launches**: 3.11, 4.15, 4.34, 4.66 s (1–4); 4.75, 6.99, 7.58, 7.75 s (5–8); 7.82, 9.97,
  10.81, 11.30 s (9–12); 11.49 s (13); 11.13 s (14) — within the client's 14 s `launch_timeout`,
  not by much.
- **Start-up**: step 1's one agent ≈ 88 s; step 2's ≈ 113 s; step 3's `born` ≈ 146 s after its
  start.
- **The 24 loads**: 34.7 and 59.0 s for the two that finished; per module max 7.4–7.9 s (`Base`).
  Last write 10.8 and 15.9 s.
- **Two VMs exited 14 (no platform)**: agents 13 and 14 died at `Redoubt::new`: the attach of
  their `/project` connection (`Tattach`, the root fid charged to `Files`) refused by walfsd, the
  account's bucket of 32 fids full. The first lease deaths with a code in this measurement,
  and a second code path to STEWARD9's `exited 1`.
- erofsd answered its sentinel at every step (36.0 MB read by the end); verityd at its 4,096th
  read: 12,040 blocks asked, 1,247 held, 4,748 blkd reads.

Two harts on rv32 are the slow corner (SCHED5's), and here the data volume's bucket fills before
the system volume's share collapses: fewer agents got any write through, and the thirteenth and
fourteenth could not attach at all.


## What the Architect decides

1. **The `Files` cap and its share rule against the account's connection count.** Today
   `files: 32` at erofsd, walfsd and littlefsd, and a share is `max(1, 32 / (holders + 1))` with
   every connection's root fid held for its life. Options the evidence leaves: (a) derive
   `files` from the manifests as `state` is (two per session, one per lease, each needing its
   root and at least one working fid, the shell's hub a few more), the bench holding every
   manifest to it; (b) leave the root fid out of the share (charge it to the bucket's total only,
   where `MAX_FIDS` and the budget bound it), so a share is working fids; (c) floor the share at
   two, not one, so a connection always has one fid beside its root (which only moves the
   collapse to about `n = 31`); (d) both (a) and (b). Whatever the rule, the supported count must
   be a stated number the case gates: today it is **13 leases of one account beside two
   sessions** by admission alone, and fewer, 7, for anything the shell's hub needs at the data
   volume.
2. **What a refused code read does to the VM.** Today it depends on where it lands: a refused attach of its `/project` exits 14 (seen); a refused
   boot-pack or first start-module read **parks the VM for good, silently** (a lease that never
   ran holds its budget to its deadline, and nothing says so); the start module's second read
   exits 1 (STEWARD9's death; explained by the code path, not reproduced here — none of this
   measurement's agents reached that exact point); a lazy load raises `undef`; the agent's own
   input read spins it. The question the node body asked — may a code read be retried past a
   bound, or must it be fatal — is now: should a refusal by admission (`too_many`, `busy`) be
   retried by the synchronous client (as the hub retries `busy` at `RETRY_US` up to
   `FILE_WAIT_US`), and must a VM that cannot start say so and exit (a code the steward's line
   carries) rather than park. The `exit-kinds` feature of this branch is the test-only form of
   the second half.
3. **The data volume under the same load is the slower path.** A 2 KB write to `/project`
   took 8–29 s at four to twelve agents on either width, two of them `etimedout` at the hub's
   10 s; the agents' input reads fail the same way, and `Redoubt.Agents.Echo` then spins by
   design. This is walfsd's and the hub's, not the read path's: a finding of its own.
4. **The supported count and the rates to state.** The read path itself is slow but never the
   cause of a death: a lease's start-up (≈ 3.8 MB of boot pack before stripping, and the start module) is 35–140 s
   beside three to eleven others on either width at one hart, 43–143 s at two; its later loads
   run at 30–85 KB/s per agent; verityd holds ≈ 10 % of the blocks asked and read-ahead gives
   ≈ 2.2 blocks a blkd read. The pages/requests VM-count term is not on this path (code reads
   are plain calls, not multiplexed sends). The weight lever would move the rates, not the edge.
5. **Phase 2's case**, as I would shape it on the ruling: this case retargeted as a gate — N
   agents of one account all loading and finishing, none refused, N the stated supported count
   on both widths at one and two harts, with the shell's probes as the account-wide check — and
   a host test of the share arithmetic at the derived cap; `exit-kinds` kept test-only or made
   the default, as ruled.
