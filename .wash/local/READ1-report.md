# READ1 report (read1-implementer-2, phase 2)

Branch `wp-READ1`, worktree `/home/mcloonan/redoubt/.worktrees/READ1`, logs under
`/home/mcloonan/redoubt/.worktrees/READ1/.tmp/READ1/` (phase 1's `runN-<width>-smpM/`, phase 2's
`host*.log`, `client*.log`, `gates*.log`, `target/jobs/<width>-<case>.log`). Head: see the
assignment's completion.

## Phase 1, summarised (`.wash/local/READ1-measurement.md`)

A lease VM's death under its account's concurrent load is **admission**, not a drain rate or a
timeout: code reads are synchronous client calls with no timeout, and what refused them was the
file servers' `Files` cap (32 per account and label set) fair-shared per connection with every
connection's **root fid** (`Tattach`'s, held for life) inside the share, so at sixteen
connections of one account at the system volume (two sessions' and fourteen leases') the share
was the root alone and no VM of the account could walk, open or read anything: every module load
refused `too many open files`. Where the refusal landed decided the death: the `/project` attach
(exit 14, seen), the boot pack or the start module's first read (a silent park for good), the
start module's second read (exit 1, STEWARD9's), a lazy load (`undef`), the agent's own input
read (a spin). Baseline: 13 leases of one account beside two sessions at the system volume, 7
before the shell's hub lost its working fid at the data volume (`emfile`). The read path is slow
but never the cause: start-up 35–143 s beside others, 30–85 KB/s per agent, verityd holding
~10 %. A 2 KB write to `/project` under the load took 8–29 s, two of seven `etimedout`.

## The ruling (`.wash/local/READ1-ruling.md`) and the derivation (`.wash/local/READ1-derivation.md`)

1. The root fid is a fixed per-connection cost outside the fair share; the share is over working
   fids, floored at one; the cap is derived from the connection mix. Signed off with the numbers:
   `files = n + w (n + 1)`, `n` the state cap's own connection sum (so roots ≤ state < files), `w`
   the most working fids one connection needs at once: erofsd 28 + 29 = **57** (w = 1: the VM's
   one reader, confirmed from the code: the session VM reaches `erofsd:system` only through the
   platform's `System` reader, one lend, one open at a time; the hub binds the home, `home=`, not
   the system volume), walfsd/littlefsd 23 + 2 × 24 = **71** (w = 2: the hub's cwd + the request
   in hand), bootfsd 23 + 24 = **47**.
2. The sync client retries an admission refusal (`too_many`, the kernel's `busy`) a bounded
   number of times with backoff; a VM that cannot complete a startup read never parks silently.
   Refined on the thread (2b'): the criterion is the console — a VM with one says why and parks as
   before (the restart-loop rule of beamlet.md, attacked by `userland-bad-start` and the
   `pack-bad-*` cases, stands); a VM with none (a lease's) exits, its code the diagnostic.
3. The slow writes are a separate finding, stated on walfsd.md. 4. The baseline and the new
   supported count are stated. 5. The gate: N agents all finishing, none refused.

## What was delivered

- **`libs/rt/src/server/admit.rs`:** `Counts { held, fixed }` per bucket and share;
  `admit_fixed`/`release_fixed`; the fair share over working units of `cap − fixed` among the
  shares holding working units; `a_root_fid_is_a_fixed_cost_outside_the_share` (the n = 16
  collapse closed; the floor; releases).
- **`libs/rt/src/server/ninep.rs`:** `Fid.root`; a connection's **first** attach is the fixed
  root, any later attach a working fid (found by the existing R26 attack test
  `minted_connections_are_admitted_and_fold_into_the_share_they_came_from`: a client attaching
  again and again would otherwise hold a bucket outside every share); every release path
  (`drop_fid`, `connection_gone`, `unreserve`, `insert_fid`'s failure) gives a root back as fixed.
  Three skeleton tests re-derived under the rule (their comments state the arithmetic).
- **The caps:** erofsd 57, walfsd 71, littlefsd 71, bootfsd 47, each `limits` comment carrying
  the derivation; bucket bytes 235,520 → 286,720, 1,617,408 → 1,697,280 (×2), 165,888 → 173,568;
  entry tests and refusal fixtures re-numbered (erofsd twelve buckets of 870 pages, bootfsd
  thirty-nine of 1,664; the refusal fixtures one page short of 420 / 255). Manifests by the entry
  rule: walfsd 4 buckets 1,792 → 2,048 (`beamlet-footprint`, `beamlet-compile-footprint`, `pipe`,
  `steward/idle`), 9 buckets 3,840 → 4,096 (`six-principals`, both entries); the image, 5- and
  6-bucket entries and littlefsd's 1,792 (1,786 by the rule) unchanged.
- **Bench host test** `the_file_servers_fid_caps_leave_every_connection_its_root_and_working_share`
  (`tools/testbench/src/case.rs`), the manifest sums factored into `what_one_account_holds()`
  shared with STEWARD7's state/in_flight test.
- **The client's bounded retry** (`libs/client/src/file.rs`, `admitted`): the attach and every
  walk, 8 tries at waits doubling from 10 ms (2.55 s), within the connection's own timeout;
  `an_admission_refusal_of_the_moment_is_tried_again` (a held fid freed inside the bound rides
  out; one held past it is the caller's `too_many` after nine walks).
- **beamlet** (`userland/otp/redoubt/src/bin/beamlet.rs`, `end`): a VM with a console says why
  and parks (unchanged for sessions and `init`'s VMs); one without exits `exit_kind(NOT_READ)`
  (1 default, 15 under `exit-kinds`). The predecessor's `exit-kinds` and `unread` naming kept.
- **Cases:** `lease-code-load` re-shaped as the gate (sixteen agents at once on the latency
  manifest, `{:gate, 16, 0, true, {:error, :enoent}}`, the steward's exit line forbidden, both
  widths at one and two harts); `lease-bad-start` (R75/R76 flip of `Elixir.Redoubt.Agents.Echo`,
  the lease VM exits and `steward: lease alice/agent-1 N of 1001/{} exited 1` is the verdict,
  `parked` forbidden).
- **Pages:** serving.md (`admit`: the fixed-cost rule, the derivation, the retry; R26's statement
  and status), erofsd.md (the cap, the account's supported count before/after, the rates),
  walfsd.md (the entry numbers, the fids, the slow-write finding under Residual risks), littlefsd.md,
  bootfsd.md, verityd.md (the rates under sixteen loaders), scheduling.md (the rv32 N = 16 row:
  the cause found and closed, `steward-latency-n16` still rv64-only until re-measured), beamlet.md
  (the two ends by the console), SECURITY.md (R26's Tested-by).

## Phase 2's own findings

1. **A `Tattach` is any number per connection.** The existing R26 attack test
   (`minted_connections_are_admitted_and_fold_into_the_share_they_came_from`) showed that a
   root-out-of-share rule applied to every attach lets an agent hold a bucket by attaching again
   and again. So only a connection's **first** attach is the fixed root; later attaches are
   working fids. Roots ≤ connections ≤ `state` holds again.
2. **The gate's first shape could not go green, for a reason off the read path.** With agents
   writing their timing files to `/project` and the shell counting `done`, at N = 4 on rv64 one
   hart 1–2 of 4 agents stopped for good inside a `File.write` whose bytes had reached walfsd (the
   file holds the lines of the very write the agent never returned from; one had loaded all 27
   modules and stalled in its last write), alive, unended, no progress in 2,000 s guest, in two
   boots with and without the client retry (`.tmp/READ1/diag4`, `diag4b`); phase 1's born-only
   agents were the same. A write's completion that never returns to the VM, past the hub's 10 s
   bound: a liveness finding of the hub's completion path or walfsd's, stated on walfsd.md
   beside decision 3, not fixed. The gate therefore keeps the data volume out of its verdict: the
   agent writes nothing and reports by its exit (return → `exited 0`, a refused load → `halt`),
   the steward's sixteen lines are the verdict, the session waits on its sixteen notices.
3. **The flipped block corrupts the volume for everyone** (erofsd's rule): `lease-bad-start`'s
   console session's shell loses modules after the lease's VM reads the flipped block; the SSH
   session needs nothing it has not loaded, and the notice lands on the idle prompt's line, so
   the case sends its next command without expecting a prompt between.
4. The first gate run also showed sixteen packs at the measured ~33 KB/s aggregate take ~1,800 s
   of guest time before any agent loads: the gate's waits are an hour for the first lease's end
   and ten minutes for each after.

## Before / after

| | before | after |
| --- | --- | --- |
| leases of one account loading at the system volume beside two sessions | 13 (the 14th: every load of the account refused) | the manifest's lease term: 16 on the latency manifest (gate), 7 on the image with margin |
| leases before the shell's hub loses its working fid at the data volume | 7 | the same term |
| a lease VM whose startup read is refused | parks silently to its deadline | exits; the steward's line says so |

## Gates (every command through `scripts/q run` / `scripts/jobs.mk`; logs in the worktree's `.tmp/READ1/`)

| gate | result |
| --- | --- |
| host tests: redoubt-rt, redoubt-client, redoubt-steward, testbench, erofsd, walfsd, littlefsd, bootfsd, fileserver (`host1.log` → 4 failures, the first attach rule and three re-derived tests; `host2.log`, `host3.log`) | green (59 test binaries ok in `host3.log`) |
| `cargo +nightly fmt --check` on the touched crates | clean after `fmt` on rt, client, steward |
| `set CASES=formatting`, `set CASES=docs` (`gates3.log`, rv64) | PASS, PASS (the docs gate first caught a bad anchor, fixed) |
| `lease-bad-start` rv64 one hart (`gates4.log`) | PASS 7.2 s |
| `lease-code-load` first shape (files written, `done` counted), rv64 one hart (`gates2.log`, `gate-rv64-1/`) | FAIL by the write-completion hang above and a wait too short: reshaped |
| diagnostics at N = 4 (`gates3.log` with retry, `gates4.log` without; `diag4/`, `diag4b/`) | 3 of 4 and 2 of 4 agents finished, the rest stopped in a write to `/project` |
| the full set (`gates5.cases`: formatting, docs, size-budget, unsafe-budget, host-tests, the 14 build cases, every steward-* boot case but latency, erofs*, walfsd*, littlefsd-boot/host-tests, verity-signed/flipped-tree, beamlet-launch, agent-login, the pack-bad-* and userland-bad-start park cases, lease-bad-start, lease-code-load; both widths; `gates5.log`) | running |

Not run, by instruction: the steward-latency cases (STEWARD12 holds the machine's slot); the whole
bench (the train's).

## Summaries checked

Searched for the changed feature's names and old claims (`32 fids`, `fid cap`, `too many open
files`, `parks`/`parked` of a VM, `13 leases`, starvation): README.md, GETTING-STARTED.md,
docs/README.md, docs/plan/*.md (m2-usable-shell.md's "starve" is the kernel lock's, unrelated),
docs/userland/agents.md ("A lease has no console": still true; the console-less end is stated on
beamlet.md, which it links), docs/servers/steward.md ("Leases": the exit line's form unchanged;
R40's list gains the no-blame test), docs/servers/init.md (the restart rule unchanged: the park
stays for a VM with a console), docs/kernel/boot.md R75 (its attack cases unchanged and still
pass), docs/testbench.md (no case-count or memory table moved), SECURITY.md (R26's and R40's
Tested-by match their rules' status lines). No other summary states the fid cap or the park.

## Commits (the squash, in this order)

1. `admission: a connection's root fid is a fixed cost outside the fair share` — libs/rt
   admit.rs + ninep.rs + the three re-derived skeleton tests + the new admit test; message: the
   found cause (sixteen connections at a cap of 32 held one fid each, the root) and the first
   attach rule.
2. `file servers: the fid caps derived from the account's connections` — the four `limits`, their
   comments, entry tests and refusal fixtures, the five manifests, the bench host test (`Holds`).
3. `client: a fid-taking request refused for the moment is tried again` — file.rs + the test +
   Served::start_with; carries `Size budget: libs/client: …`.
4. `beamlet: a VM with no console that cannot start exits, its code the diagnostic` — beamlet.rs,
   lib.rs's public kinds, lease-bad-start, beamlet.md, the steward no-blame test, R40's lists.
5. `testbench: sixteen agents of one account load code at once and every one finishes` — the
   gate case, echo.ex, the predecessor's measurement (its `unread` naming and `exit-kinds` are in
   lib.rs/userland.rs: fold into 4 or here).
6. `docs: the file servers' fid caps, the account's supported count and the data volume's write
   findings` — serving.md, erofsd.md, walfsd.md, littlefsd.md, bootfsd.md, verityd.md,
   scheduling.md, SECURITY.md.

`Size budget: libs/rt: …` goes on commit 1's message (the commit that raises the ceiling must
carry it); `Size budget: libs/client: …` on commit 3's.
