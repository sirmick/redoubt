# B92 report: a whole bench holds the machine's QEMU lock; every other boot waits for it

Branch `wp-B92`, worktree `.worktrees/B92`, head `44a929d3a` (one commit on origin/main 5ced0b2b3).
Reviewer kernel-red-4. Tier B tooling, size S.

## Found cause

Train 82's three walfsd timed steps failed under the whole bench beside three members' boots and
passed alone twice on both widths. Nothing in the tooling held the condition the timed bounds are
stated under (one whole bench at a time, QEMU its alone for the run; the owner's rule of
2026-10-05): a train and a member's boot shared the machine freely, each under its own q lease.

## Delivered

- `scripts/q`
  - `--share NAME`: a shared take of a named lock beside the exclusive `--lock NAME`. Sharers run
    beside each other; an exclusive taker waits for the holder and every sharer; from the moment
    an exclusive taker waits, no new sharer starts ahead of it (writers first, so a stream of
    single boots cannot hold a whole bench off). Daemon: `locks` (name → exclusive holder) and
    `shared` (name → sharers), `lock_blocker`, `take`/`release`.
  - `--cores 0`: a lock-only lease; no core, no taskset pin. The client's `CARGO_BUILD_JOBS` stays ≥1.
  - One stderr line to a job held on a lock, naming the blocker (tenant, name, cwd) and since when it
    has held, or waited for, the lock; sent once by the daemon only to clients that ask (`notify`),
    so an older client against the new daemon still reads its lease as the first line.
  - `q ls`: `lock qemu: held by <tenant> '<name>' (<cwd>) since HH:MM:SS (Ns)` / `shared by N`.
  - Adoption across a daemon restart: `Q_LOCK` / `Q_SHARE` in the job's environment (set by the
    client, never a parent's) are adopted as cores and ports are; a lock-only job (no pin) is known by
    its lock; the hold's start is the process's start time from /proc. Adoption is scoped to this
    daemon's socket (`REDOUBT_Q` in the job's environ), so a q-test daemon and the machine's never
    adopt each other's jobs (before, a test daemon could not see the real one's pinned jobs only
    because of its core set; with lock-only, unpinned jobs the socket check is needed).
  - Old daemon + new client with `--share`: the job runs unlocked and the client says so on stderr
    (`restart the daemon: q stop, q daemon`), as the `--ports` fallback does. Seen live: the machine's
    daemon predates the change, so `rv64/rng` printed that line and ran.
- `scripts/jobs.mk`
  - `whole`: cases-*/quiet-*/all-* among the goals, or `set` whose CASES hold every case of the
    boot, quiet or net class. `share`: `--share qemu` for a case with an `arch`, nothing under a
    whole run (it holds the lock) or for a case that boots nothing (`hostonly`: every host-tests /
    Elixir / build / fmt / docs case has no `arch`).
  - `qemu-held` target: `q run --cores 0 --lock qemu --name "bench <worktree>" -- bash -c 'echo held;
    while kill -0 $PPID…; do sleep 1; done'`, read through a process substitution so the recipe
    returns once `held` is printed (the lock is make's), and the holder outlives nothing but the
    make (its `$PPID`): the lease, so the hold, ends within a second of make's exit by any path
    (killed, dead train), no trap, no file. Under a whole run every case target has `| qemu-held`
    as an order-only prerequisite, so no case starts before the hold.
  - The case recipe echoes the q waiting line (if any) with the case's result, since q's stderr goes
    to the case log.
  - Class macros (boot/quiet/net) pass `$(call share,$(@F))`; fanned/bounded take no lock.
- `launch`: the `--system` boot's lease takes `--share qemu`; comment and notice updated.
- `scripts/q-test`: 15 new checks (29 total): two sharers beside each other; an exclusive taker waits
  for them; a sharer arriving while it waits waits behind it and is told (`waited for it since`);
  the exclusive runs once the sharers end, on no core, named by `q ls`; a second exclusive waits
  and is told (`held it since`); a restarted daemon adopts the hold and the waiters still wait; the
  hold's end releases the waiters in turn; every lock and port freed. Cleanup kills every job a
  failed run started (a leftover was adopted by the next run, which is how the socket scoping was
  found).
- Pages: `docs/testbench.md` — the `scripts/q` paragraph (scheduler section) states the lock, who
  takes it how, the hold's end, the single boots' wait and line, the exemptions, adoption, the old
  daemon's notice, and q-test; the sweep line gains `--share qemu`; one clause beside the host-clock
  verdict rule ("the rule asks that no other invocation and no build run beside it") points to it.
  `GETTING-STARTED.md` — launch's boot takes the lock shared. `scripts/jobs.mk` header paragraph.

B91 (wp-B91 d8b73bc5a) already carries the condition sentence beside the timed cases' classes (the
"A boot case without icount" sentence); mine is placed at a different line (the host-clock verdict
rule, ~20 lines below), so the two should merge without conflict. Not placed twice.

## Tests run (exact commands, exit codes)

- `python3 -u scripts/q-test` (REDOUBT_TMP=.tmp/B92) → rc 0, `PASS q-test`, 29 ok lines
  (`.tmp/B92/q-test.out`).
- `make -n -f scripts/jobs.mk all-rv64`: the hold recipe present, 0 `--share qemu`;
  `make -n … set CASES=smp-evict`: `--cores 2 --share qemu` both widths;
  `set CASES="docs rt-host-tests formatting"`: no lock; `set CASES="<every case>"`: whole (hold).
- `make -f scripts/jobs.mk prebuilt` (real daemon) → rc 0 (rv64 363 cases, rv32 349, 0 failed).
- `.tmp/B92/check.sh rng` (a daemon of its own on REDOUBT_Q=.tmp/B92/check-q.sock, cores 17-19,
  the real prebuilt) → `PASS check.sh rng` (`.tmp/B92/check.log`, `check-*.out`, `check-q.sock.log`):
  1. `q run --cores 0 --lock qemu --name 'bench fake-train' -- sleep 25` holds the lock on no core;
  2. `make rv64/rng` and `make rv32/rng` started under it wait (q ls: waiting 2, share=qemu); both
     logs and results carry `q: rv64/rng waits for the qemu lock: B92 'bench fake-train' (…/B92) has
     held it since …; it runs when the lock is free`, and both PASS (0.4 s) once the hold ends,
     waited=24.4 s in the daemon's log;
  3. a second `--lock qemu` job waits and is told `has held it since …`, runs once the hold ends;
  4. `make qemu-held all-rv64 -o cases-rv64 -o quiet-rv64` prints `qemu lock held for this run (…)`
     and the lock is gone from `q ls` within 2.5 s of make's exit.
- `make -k -f scripts/jobs.mk set CASES="formatting docs"` (real daemon) → rc 0: PASS formatting
  23.2 s, PASS docs 4.8 s.
- `make -k -f scripts/jobs.mk set CASES=rng` (real daemon) → rc 0, both widths PASS; the log carries
  the old-daemon notice.

Not run: a whole bench (not this package's gate; the train recipe's commands are unchanged).

## The train recipe

`run.sh` need not change: `make -k -j8 -f scripts/jobs.mk all-rv64 all-rv32` is one make invocation;
`qemu-held` runs once in it (a prerequisite's recipe runs once), rv64 and rv32 cases interleave under
-j8 as before, and the hold ends with that make. `-k`: if the hold recipe failed (no q), every case
is skipped as "not remade", as it would be without q anyway.

**At merge: the machine's daemon must be restarted (`scripts/q stop; scripts/q daemon …`) for the
lock to exist; the new daemon adopts the jobs running under the old one. Until then every `--share`
job prints the notice and runs unlocked (today's behaviour). Members' worktrees from before the merge
run their own old `scripts/q` client and `jobs.mk`: they take no lock and are not blocked by a train's
hold; only rebased worktrees wait.**

## Summaries checked

- `README.md`: no mention of q or jobs.mk; no change.
- `GETTING-STARTED.md`: launch's boot paragraph updated; lines 66-67 (q daemon once per machine) still true.
- `docs/testbench.md`: updated as above; "Case targets may run at once … the leases, not make, keep
  the host from being oversubscribed" still true (the lock is a lease's).
- `docs/kernel/scheduling.md:124` ("beside a whole bench's load"): a measurement note; no change.
- `docs/plan/*` milestone pages, `CONTRIBUTING.md`: no mention; no change.
- Pages carry no dates and no package ids; case files untouched.

## Open risks / residuals

- Writer preference means a member's single boot queued behind a *waiting* whole bench waits for
  the bench's whole run (intended: the bench goes first). A boot by hand that bypasses q, or an old
  client, is not held; the rule is the pages', the tooling holds it for every q user.
- Nesting: a `--share` taker inside a job holding the lock shared would wait on its own outer hold
  if an exclusive taker is waiting; no such nesting exists (fanned host-tests jobs take no lock);
  stated in q's docstring.
- The `set` whole-ness test is "every case of the boot, quiet or net class"; a set covering most of
  a class is single boots (shared), by design.
- `.tmp/B92/stop-my-q-jobs.py`: a note for the record — a shell loop matching `scripts/q run` in
  /proc cmdlines killed my own shell once (the memory's trap by another route); kill by exact argv.
