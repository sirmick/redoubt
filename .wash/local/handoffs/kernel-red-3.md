# kernel-red-3 handoff (2026-10-19, at 70 % context; READ1's verdict delivered)

## Branch state and traps first

Reviewer: no branch, no worktree of my own, nothing to commit; I never wrote a file. I read in the implementers' worktrees (`.worktrees/<PKG>`) with `git diff <base> <head>`, `git show`, grep, sed, `cut -c1-200`; ran host tests only when the orchestrator offered (BENCH1: `scripts/q run --cores 4 -- cargo test -p testbench` in the worktree; the firmware env vars were not needed for host tests).

Every verdict I gave is delivered; nothing is half-reviewed. The last: READ1 (wp-READ1 95f95f018 on c01146946) Merge verdict OK, sent to the orchestrator 2026-10-19 ~19:40Z local; the orchestrator merges it.

Traps found this session (each fixed by a fold; the rule stands):

- **"One blkd call" (B88).** verityd's binary lends 2 pages (`Blkd::new(_, 2)`): `per_call` = (8192-512)/512 = 15 sectors, so an 8-block read-ahead is 5 wire calls; the bench's and boot-stats' "blkd reads" count `Tree::read`, not calls. A page claim about call counts must be checked against `libs/fileserver/src/range.rs` `read_at` (chunks by per_call).
- **"The hub re-asks" (IPD1).** The beamlet hub maps a server's Rerror timeout to `etimedout` for the Erlang caller (`userland/otp/redoubt/src/files.rs` `Name::Timeout => FileError::Etimedout`); it does not retry. sshd's listener does (`Again::Now`). A page saying "the client asks again" must name which client.
- **Rule F vs. what a canary proves (ATK10).** Canaries unchanged judge writes; a read or listing that succeeded leaves them as they are. State on the row what rests on structure (the lease's namespace, host-tested) and what the case judges.
- **The capture is a bound, not a proof (BEAM5, ATK13).** No negative shows an unrefused SYN to 127.x or ipd's own address reaching the wire; the verdict carrier for a refused loopback connect is the refusal name as an expect, plus the peer's count. Ask for `self_forbidden` so the capture bounds the wire.
- **Numbers on pages are the head's (B78, B80, B88).** A ratio or a width quoted on a page must be recomputed from the kept logs: B78's "five times" was the old indexer's (3.2x at the head); B80's "within 100 us" held for one side only (the other side's clusters 2.3/2.8 ms apart); B88's "one call". Recompute before accepting.
- **Stale comments count (STEWARD8, IPD1, B77).** A constant's move must be grepped across the tree for restatements in doc comments (pool.rs:13 said "5 s" after MAX_CALL_MS moved to 12 then 14 s; testbench.md:358 said "keep the net lock" after B77).
- **A measured bound beaten by the verification run moves (STEWARD8).** The derivation's worst (5.851 s) was beaten by the tree's own verification (6.64 s): 2x worst rounded up moved 12 -> 14 s; constant, fixture (14000/14001), vm test (14000000 us), pool.rs comment, beamlet.md row, system.rs doc all restated.
- **Gate provenance (BEAM5, ATK10).** Job logs under target/jobs are overwritten per run; a "3x" claim is verifiable only from kept rep-*/run-* logs (B89 and BENCH1 kept them; ATK10 did not). Check the kept run's time against the commit time and the mtimes of the files the boot cases build (BEAM5: the .beam predated the run, the tomls were rewritten by the rebase).
- **Fixture .beam files are committed** (vm/tests/fixtures/*.beam; prior commits did the same). Check the .beam postdates the .erl and the host test ran after the rebuild.
- **The result size.** A completion body or a message to the orchestrator over 2000 bytes bounces and costs a full resend (I lost ~8 resends). Draft under 1800 bytes the first time; one head line `Merge verdict: OK | OK with notes | NOT OK`; base, head, paths read; findings P1/P2/P3.
- **Reply shape.** A review's result goes in the assignment's completion (`member_update assignment_results`) AND a message to the orchestrator; a renewal (an amend after a fold) is a one-line `Renewal verdict: OK/not` by `message_send` with `reply_to`, then `member_update waiting`. When the sender of a renewal note is the implementer, still answer the orchestrator (the merge gate). `cc` of a retired implementer bounces: drop it.

## Verdicts in force (mine, this session)

B77 OK (ports lease, renewed at e7b765b91); B78 OK (search index, renewed 77269612d); B79 OK (vault disk, renewed 179f2d050); B80 OK (one-hart icount, renewed 338d014f6); B81 OK (held clock, system.rs); B82 OK (deadline-in-launch case); B83 OK (ended step); B85 OK (held clock, console.rs; rebased bf29a2d5b); B86 OK (port slices); B87 OK with notes (oracle: a deadline-preempted device interrupt, 4d09f2e89; condition met); B88 OK with notes (verityd cache/read-ahead, renewed 0a855517c); IPD1 OK (8 s park bound, renewed c994a9003); BEAM5 OK with notes (gen_tcp over /net, renewed 7038fc500); ATK10 OK with notes (renewed 0c8277f2e, rebased f0b8e5040); ATK12 OK with notes (two P3s, no renewal asked); ATK13 OK with notes (renewed dd1cdf959 on 5923f7bc8); STEWARD8's ceiling commit OK (final 5deb51c62 at 14 s; the client commit and the measurement are steward-red-5's); B89 OK; BENCH1 OK; READ1 OK.

## The two open arms I may have been asked to review

- **KILL1** (the Architect's `plan-2026-10-20-kill1.toml`): in every completed boot of `lease-code-load` (both widths, one and two harts) fifteen of sixteen lease VMs exit 0 and the sixteenth is `was killed 0` the instant the lease that ended just before it is destroyed, with no `LeaseEnded` of its own first. R39's failure. Suspects named in `.wash/local/READ1-gate-finding.md`: the steward's `destroy_budget` reaching another lease's budget (a token/handle to the wrong one; cf. the recycled-scope-index tests under steward.md "Failure and restart") or the kernel charging one VM's process object to another's budget (R10's "same doomed subtree" clause, budget.rs ~1383). Evidence: `/home/mcloonan/redoubt/.tmp/READ1-evidence/gate10/` and `gate13/` (console logs; grep `Terminating process with PID`, `was killed`, `LeaseEnded`). The acceptance is `lease-code-load` going into the whole run (`whole_run = false` today with the mechanism in its comment). Review rule: the fix must name which of the two it was with a trace line, and the case must pass 3x both widths with no non-zero end; a kernel fix is Tier A and needs its negative shown once.
- **WRITE1** (queued): the data volume's write path. Two findings stated on walfsd.md "Residual risks": a 2 KB write to `/project` under the account's load takes 8-29 s, two of seven `etimedout` at the hub's 10 s; and a write whose bytes reached walfsd never returned to the VM (the agent stopped for good inside `File.write`, alive, no progress in 2,000 s guest; `.worktrees/READ1/.tmp/READ1/diag4`, `diag4b`). The second is a liveness finding of the hub's completion path or walfsd's. Review rule: the hang must be reproduced and named (hub or walfsd) before a throughput bound is set; the bound, if any, derived as 2x the worst measured per width rounded up, stated on the page with its arithmetic; held-clock host tests for the hub side (B81/B85's shape).

## Standing facts of the tree I relied on

- The bench: `scripts/q` leases cores and ports (`q run --cores N --ports N`; `REDOUBT_PORTS=lo-hi`; a sweep slices the lease per `--jobs`); `scripts/jobs.mk` classes boot/quiet/net/fanned/bounded; `make -f scripts/jobs.mk set CASES="..."` runs cases both widths; job logs `target/jobs/<w>-<case>.log` (overwritten), failing boots' console/session logs kept beside them; `--keep-failed`; the oracle's kept-log re-judge `SCHED_ORACLE_LOG=... cargo test -p testbench a_kept_log_judged_again -- --ignored`. Scratch is `$REDOUBT_TMP` = `<checkout>/.tmp` (never /tmp); implementers keep evidence under `.worktrees/<PKG>/.tmp/<PKG>/` or `/home/mcloonan/redoubt/.tmp/<PKG>/` (look in both).
- Case rules: `icount = "shift=3,sleep=off"` default for boot cases; a disk keeps sleep on; share/latency reads at 2+ harts under icount are suspect of QEMU's turn order (K31): twins under mttcg on the quiet class judge. A timed claim in a session is the in-guest ms by the expect's digits, bound = 2x worst per width rounded up to a second, `within = N` the ssh margin stated as no part of the claim (ATK11's shape). `smp` is a list or per width (`{ rv64 = [1, 2], rv32 = [1] }`, BENCH1). Dials order on console events (`after`, one-shot flags). `distinct` captures must differ. Status-line counts must equal their lists (doccheck C7 ties SECURITY rows to rule-section status lines).
- Rule F: a verdict is the system's (console lines of init/steward/sshd, the bench's peer counts and capture, the victim's own session), never the attacker's or the agent's output; an agent's file is progress at most.
- Fake kernel held clock (`Fake::hold_clock`/`tick`): a tick waits for every other fake thread to be in a wait (STUCK 60 s fails loudly); tests serialise with a static `turn()` mutex; the console fixture's feed thread is counted only while it wakes the server.
- Admission (READ1): a connection's root fid is a fixed cost (`admit_fixed`), the share over working fids floored at one, caps `files = n + w(n+1)` (erofsd 57, walfsd/littlefsd 71, bootfsd 47); the client retries too_many/busy 8 times from 10 ms doubling within the connection's timeout.
- beamlet: MAX_CALL_MS = 14 s = the client's `launch_timeout/0` (STEWARD8); gen_tcp over the session's /net when the namespace has one, the in-VM loopback stand-in only when `/net/tcp/clone` answers enoent (BEAM5); a lease has no /net; a VM with no console that cannot start exits (READ1).
- ipd: DATA_WAIT_US = CTL_WAIT_US = 8 s below the hub's FILE_WAIT_US 10 s (IPD1); the hub answers its caller etimedout on a server timeout.
- verityd: data cache sized from the manifest's heap (cap 118, reserve 56, floor 4), read-ahead of a request's cold run, one read = up to five blkd calls at the two-page lend (B88).
- The oracle accepts a device interrupt without a claim only with a destruction inside the entry and an `O 2` return (B87); its parse allowlist refuses `@` (block-trace, a test-only kernel feature no case sets).
- Memory budget: `heap_fits = true` passes a server whose peak leaves an eighth of its cap (walfsd-pools); otherwise cap >= 2x peak.

## Flake classes seen (and the fix shape each took)

- Host-clock waits in fake-kernel host tests under bench load (B81 system.rs, B85 console.rs): fixed by the held clock; residual: the 60 s stuck guard under >= 4 concurrent copies of beamlet-lookup-host on one 4-core lease (pre-existing, not the bench's one copy).
- Host-clock ratios at smp=1 across migrating E-cores (B80): judged under icount; a one-hart case needs `keep_smp` and its reason.
- Burst dials reordered by a late SYN (B89): order dials on console events.
- Port collisions in sweeps and between net cases (B77/B86): leases and slices, never the OS's pick under q.
- A boot's expect of a line the end destroys with its relay (B83): `ended` step keeps the input open; marker on its own line matched with the prompt.
- Admission starvation misread as a drain/timeout (READ1): the Files cap and the root fid in the share.
- kernel-deadline leases shorter than the launch under load (B82, STEWARD8): a measured launch bound (14 s) and a case lease sized from it (lease-code-load's follow-up).
- lease-code-load's rv64 one-hart SSH close by the guest while idle (once; not reproduced): a flake of that boot.
- The pre-existing `field harts is never read` warning in testbench's build: not any package's.

## What consumed my context

Whole-file diffs of large files (sched_oracle.rs, beamlet_tcp.erl, system.rs), repeated oversize result bounces, and re-reading reports. Cut diffs with `grep "^[+-]" | cut -c1-200`, read reports with `cut -c1-250`, grep logs (never cat), draft results under 1800 bytes.
