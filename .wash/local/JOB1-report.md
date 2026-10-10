# JOB1 report (job1-implementer-2, 2026-10-09)

Branch wp-JOB1 in /home/mcloonan/redoubt/.worktrees/JOB1. Base main f5316e38c, head 29263f5c3.

## Commits

- 0db2e731a shell: the interrupt ends the line being evaluated, and jobs
- 030d6c9fa shell: follow(path), the lines added to a file until the interrupt
- 29263f5c3 tests, docs: jobs and the interrupt on the machine, on both widths

## Built, against .wash/local/JOB1-design.md

| Deliverable | Where |
| --- | --- |
| Interrupt (0x03, 0x1C) while a line evaluates; under an open feed it ends the line and closes the feed; with a screen in front unchanged | `Driver` (`evaluating`, `interrupt_line`), `Evaluator.eval` registration by token |
| The line killed with `:kill`, bindings kept, wait ≤2 s for the line's foreground jobs (`Jobs.settle`), a line printed if still ending | `Evaluator.interrupted/2` |
| Job registry, started at the first job | `Redoubt.Jobs` |
| `Job.start/2`, `bg()`, `Job.status/kill/await`, `jobs()`, `Job` aliased at the prompt | `Redoubt.Job`, `Session`, `Shell.prompt_env` |
| Background jobs: no console, unaffected by interrupt/line end, stdout and stderr kept 64 KiB each, dropped bytes counted | `Pipeline.start/2`, `keep_loop` |
| `follow(path)` (500 ms poll; `:truncated`/`:removed`) | `Redoubt.Util` |
| Attack lines as cases: "An interrupted line loses only itself" → job-interrupt-line; "A job ends, and only it" → job-kill; "No program swallows the interrupt" → job-interrupt-native; SSH break + follow → job-interrupt-ssh (INT stays sshd-loopback-interrupt) | tests/*.toml, rv64 + rv32 |
| pipe-stage `spin` | tests/beamlet-programs/src/bin/pipe-stage.rs |
| Pages: shell.md (Interrupting and killing jobs, Session commands: follow, Native programs and pipes: `bg` as the example, `Redoubt.Cmd` still not built), native.md (Killing a job), m2-usable-shell.md Progress | docs |

Not built, as the design rules: `Redoubt.Cmd` (design Q4, later), fg/bg moving, Ctrl+Z.

## This session's change

- Rebased the three commits onto f5316e38c: clean. Main's driver change (Term.slices) and
  JOB1's driver change touch different clauses.
- Review fix, folded into 0db2e731a: `Job.await(job, t)` timed out client-side and left its
  caller in the job's waiters; a job ending later replied to that stale caller and was deleted,
  so the next `await` gave `{:error, :unknown}` and the output was lost. The wait's bound is now
  the server's (`{:gave_up, id, from}` drops the waiter and replies `:timeout`); new host test
  "a job that ends after an await gave up keeps its result for the next" (fails on the old code:
  status `:unknown`).

## Gates (logs under /home/mcloonan/redoubt/.tmp/JOB1/)

On a97526fb2 (rebased, before the fix), gates3/:
- `q run --cores 8 -- ./test-shell`: rc 0 (beamlet 314 tests, 0 failures; beam, fake kernel ok)
- `make -f scripts/jobs.mk prebuilt`: rc 0 (rv64 266, rv32 252 cases built)
- `make -f scripts/jobs.mk set CASES="job-interrupt-line job-interrupt-native job-kill
  job-interrupt-ssh pipe-carries pipe-hostile-output pipe-interrupted pipe-never-reads
  pipe-no-authority shell-long-output beamlet-footprint docs size-budget formatting"`: rc 0,
  25 PASS (both widths; docs, size-budget, formatting rv64 only)
- `make -f scripts/jobs.mk set CASES=sshd-loopback-interrupt` (net lock via jobs.mk): rc 0

On 29263f5c3 (head), test-shell-3.txt and gates4/:
- `./test-shell`: rc 0 (315 tests, formatting ok)
- prebuilt rc 0; set "job-interrupt-line job-interrupt-native job-kill job-interrupt-ssh
  beamlet-footprint size-budget docs formatting": rc 0, 13 PASS

Not rerun on the head: pipe-*, shell-long-output, sshd-loopback-interrupt (the fix touches only
`Redoubt.Jobs`, which none of them call except through Pipeline's add/ended, unchanged).
Not run: the full bench, unsafe-budget (no Rust unsafe added; pipe-stage `spin` is safe Rust).

## Attack verdicts (rule F)

Every verdict is the session's own printed output, read from the kernel where budgets are
concerned (`{:base, ...}`, usage back to before the line, `Pipes.usage` `{error, not_running}`);
the hostile stage (spin, a stage holding the feed) prints nothing that is judged.

## Summaries checked

- README.md:35 (lists jobs among M2's shell): no change, a goal statement.
- docs/README.md:79, docs/userland/README.md:20,22,62: index lines, unchanged and true.
- docs/plan/m2-usable-shell.md: Progress updated in 29263f5c3; attack lines 27-38 unchanged and
  now each a case; Remaining work step 4 unchanged (step list, no status).
- docs/servers/sshd.md:185-195 (INT/break become 0x1C): true, unchanged.
- docs/SECURITY.md:262-267 (links Interrupting and killing jobs): unchanged; no new register row,
  Job.kill adds no authority.
- GETTING-STARTED.md: no jobs claim.

## Open risks

- `follow` holds an unended line whole until its newline: bounded only by the line's heap limit,
  which ends the line, not the session.
- Design questions 1-6 were answered before this session (the branch follows 1 yes, 2 yes,
  3 yes, 4 later, 5 as listed, 6 Tier A); I found no QA thread recording them.

Next: review by steward-red (Tier A).

## Fix round 1 (steward-red OK with notes on 29263f5c3)

Head cae21afea, base still f5316e38c (main has since moved to d2f6e523e, a testbench-only
change; not rebased onto it, so the range-diff is like for like):
cb156b7b3 interrupt+jobs, 673038eb0 follow, cae21afea cases+pages.

- P3-1, folded into cb156b7b3: `Redoubt.Jobs` keeps at most 16 ended background jobs nobody has
  awaited (`@kept_ended`); past them the earliest started is dropped whole and counted
  (`Jobs.dropped/0`), and `jobs()` ends with "(N ended background jobs dropped unread)". Kept
  results are so bounded at 16 × 128 KiB. Host test: "past 16 ended background jobs unread, the
  earliest is dropped and counted".
- P3-2, folded into 673038eb0: `follow` holds at most 4 KiB of a line not yet ended
  (`@follow_held`); past that it prints what it holds as a line and goes on, so each poll splits
  at most 4 KiB + 64 KiB. Host test: "follow prints a line not yet ended as it stands once it
  passes 4 KiB".
- shell.md (cae21afea): the follow row and "In the background" state both bounds; commit
  messages of both code commits say them.

Gates on cae21afea (logs .tmp/JOB1/gates5/):
- `q run --cores 8 -- ./test-shell`: rc 0 (317 tests, formatting ok)
- `make -f scripts/jobs.mk prebuilt`: rc 0
- `make -f scripts/jobs.mk set CASES="job-interrupt-line job-interrupt-native job-kill
  job-interrupt-ssh beamlet-footprint size-budget docs formatting"`: rc 0, 13 PASS (four job-*
  and footprint on rv64 and rv32)
