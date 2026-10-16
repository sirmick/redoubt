# STEWARD3A handoff (steward3a-implementer, 2026-10-15, checkpoint at 70 % context)

## Branch state

- `wp-STEWARD3A` at **98a166314**, one commit on main **43588e0d6** (train 37; rebased already,
  two conflicts resolved: docs/plan/m3-agents.md merged both halves, bin/steward.rs kept main's
  stack comment + my `AGENT` const). Tree clean. Worktree `/home/mcloonan/redoubt/.worktrees/STEWARD3A`.
  The commit is the whole package minus Q3 (below); its message describes it. Amend rather than
  add commits; history squashes at acceptance.
- `target/prebuilt` is current with 98a166314's *sources* (built before the amend, same content).
- A background `make -k -j4 set` of the gate cases minus the three lease cases was started at
  ~17:50 on both widths; its log is `.tmp/STEWARD3A/cases5.log` (nothing finished at handoff).
  It may still be running: check `scripts/q` / the log before building; **never edit the tree
  while it runs** (prebuilt cases fail "tree changed").

## Traps

1. **A lease's VM dies with exit 1, silently** (the open item, Q3): beamlet's platform opens
   `/dev/cons` in `Redoubt::new` (`userland/otp/redoubt/src/lib.rs:291`) and `run` returns 1 with
   nothing to say. The Architect ruled (a): make the console optional (details below). Until
   built, `steward-lease-ends`, `steward-end-lease`, `steward-sub-agent-dies-with-agent` fail
   (the lease starts, the notice reaches alice's shell, the agent writes nothing).
2. `Recorder` in `servers/steward/tests/steward.rs`: a session's launch makes the `notices`
   handle *before* the pid, so `k.made` after a login is still the pid; the `kept:` indexes in
   older tests shifted by one (now 18).
3. The steward's usage lines count **carved** limits: a lease shows as 22 processes (its 2)
   over alice's 20, a sponsored session as 10; a sub-budget inside a lease is invisible there.
4. `DestroyBudget` after the kernel's deadline: `drive.rs` marks the pid gone on `Exited` before
   deciding, so `BadHandle` is tolerated only then (the ruling's "bind it to the event").
5. The oracle's `run-traces` builds beamlet (minutes); `mktemp` needs TMPDIR (q sets it).
6. Line numbers in a test manifest shift when a principal line is added (console test: line 7).
7. `shell-compile.toml` (AGT3, now in main) expected `carved users/agent`: fixed to alice's
   share, 20 processes (agent alone beside the console).
8. `cargo fmt` must be `cargo +nightly fmt -p <crate>`; stable rustfmt ignores the repo's options.
9. Run dirs under `target/testbench/run-*` are pruned: copy a case's logs right after it ends
   (`.tmp/STEWARD3A/lease-ends-rv64/` has the last `steward-lease-ends` transcripts).

## Rulings (QA threads, all answered; files in .wash/local)

- `STEWARD3A-notice` (Q1): (a) then **amended (a2)**: the steward makes the endpoint pair at a
  session's launch, hands the VM the receive right as the named handle `notices`, keeps the send
  right (not stamped with the scope), closes it at `Forget`; wire opcode 17 `lease_ended(lease,
  name)` is a send, never a call; no new native. Built.
- `STEWARD3A-sponsored-login` (Q2): **(A)**: `sponsor=` on the principal line; a sponsored
  principal has no top; its login is a session carved from the sponsor's unlabelled sub-budget
  with deadline MAX_LEASE; EndLease rows in the session machine; notify_sponsor on every leave;
  labelled login refused; names `<principal>/<name>`; core, oracle, model mine; carol sponsored
  in the model (no new draw); image folds agent's budget into alice's; sizes.agent measured.
  Built. **Q3 (same thread, 2026-10-15 ~18:05): (a)**: the platform tolerates no console:
  `Console::open` optional in `Redoubt::new`; with none, console writes dropped, reads answer
  end of input, size the default 80x24; `Arc<Console>` through the hub, the io and the size
  query becomes an Option, one path; a host test runs a VM with no `/dev/cons` through its
  module to exit; beamlet.md's platform table says `/dev/cons` is optional and what its absence
  means; agents.md says an agent's output has nowhere to go but `/project/out`. (b) the UART
  refused. Keep the steward's exit line but as `steward: lease NAME of ACCOUNT/{} exited CODE`
  with NAME the notice's name (`alice/agent-1`), and add the row to steward.md's console-line
  table. Also: quote the measured numbers in the report (the notice 0.34 s after the lease
  started; alice's shell printed `[lease alice/agent-1 ended]`).
- The questions file: `.wash/local/STEWARD3A-questions.md`; rulings:
  `STEWARD3A-Q1-ruling.md`, `STEWARD3A-Q2-ruling.md` (Q3 appended).

## What is built (all in 98a166314)

Server: `protocol.rs` (start_agent/end_lease bound, `ahead()`, lease_ended Class::None, a
Class::None caller refused), `drive.rs` (owner-aware connect/launch, notices map + `notify` +
`lease_name`, Exited marks pid gone, DestroyBudget tolerance), `own.rs` (`Child`, lease binds
slots 0 /boot, 1 `~/project` at /project via the home carve `below`, 5 system; `lease_args`),
`bin/steward.rs` (AGENT module, notices endpoint pair at launch, `notify` one page one send
NOTICE_US 100 ms, accounts map from `inspect::fixed` (all principals), exit-cause line
`steward: Lease ID of ACCOUNT/{} exited CODE` — **rename per Q3 to `lease NAME`** using
`steward.lease_name(object.id)`), `lib.rs` (iterate named carves).
Core: manifest.rs sponsor field/grammar/writer/Fixed::new/carves(name, skips sponsored);
effects.rs carve_session + notify_sponsor; guards.rs sponsor_of, sponsor_set; edges.rs
end_lease by kind; tables/session.md +9 rows; gen registry; generated files; trace
`sponsored.trace`; trace output `sponsor=`; oracle guards/effects/steward/trace/gen.
Model: 2 mutations (caught seeds 274, 6), carol sponsored, P1/P13/Usage, reach table 146/150.
init: manifest `sponsor`, `budget` optional (required xor sponsor), checks, holds/contexts_of via
the sponsor, steward_lines top 0,0,0 for sponsored. Wire: opcode 17 lease_ended (regenerated).
Image: agent `sponsor: "alice"` no budget; alice 81,920/64; sizes.agent 2,560/2 (measured heap
peak 716 rv64 / 691 rv32; `beamlet-agent-footprint` case, both widths PASS); README arithmetic;
`tests/data/boot-profile/manifest-unverified.json` mirrors. Shell: `Redoubt.Shell.Notices`
(serves `notices`, prints `[lease NAME ended]`), `Redoubt.Agents.Echo` (echo/spin/sub MS; on a
missing input writes the error and waits; finish reads the file back). Cases:
steward-lease-ends, steward-end-lease (bob names an unknowable id → unknown; vault → not_sponsor),
steward-sub-agent-dies-with-agent, beamlet-agent-footprint; agent-login and shell-compile
expectations moved to alice's share; size-budget libs/wire ceiling 3936. Docs: steward.md
(manifest lines, protocol, lease ops, Leases built + residuals, Principals, R39, guards table
rows), agents.md, sessions.md, shell.md (notice line in the machine mode), init.md, beamlet.md,
model.md, SECURITY.md R39, m3-agents.md, userland/README.md, GETTING-STARTED, image/README,
wire table intro. `cargo testbench docs` PASS at 7b9162414 (same docs now).

## Gates run (results in `.wash/local/STEWARD3A-report.md`, partly filled)

Green on the host: steward core/gen/trace/server/init/wire-gen tests; rv64+rv32 checks;
model families (14.6 s on 4 threads), the two mutations, reach reproduction; oracle run-traces
(all hand traces ok, sponsored included, no untaken row); nightly fmt; mix format; shell mix
test 308 passed; docs. Machine (first set, rv64): PASS agent-login? **no**: it failed then
(accounts map) — fixed in 98a166314, not yet rerun; PASS: launch-system, steward-ssh-two-
principals, steward-home-quota, image-disk, beamlet-footprint, beamlet-agent-footprint,
shell-compile-module, steward-host-tests, steward-model-host-tests, model-host-tests,
init-host-tests, wire-host-tests, elixir-oracles, formatting, unsafe-budget, docs; FAIL then
(fixed since, not rerun): size-budget (ceiling), shell-compile (expectations); FAIL (Q3): the
three lease cases. rv32 of the set never ran (the 2 h job limit).

## Next steps, exactly

1. Wait for/kill the background set (`cases5.log`); read its PASS/FAIL lines.
2. Build Q3 (a) in `userland/otp/redoubt/src/lib.rs`: `console: Option<Arc<Console>>`,
   `cons: Option<ConsoleIo>` (25 `self.cons` sites: idle, console_write, console_listening,
   console_read, start_reading (skip `resizes.start` without a console), dispatch's `take`,
   the io report lines), `console_size` → None without one, `run()` lines 708/733 (`say` only
   with a console). Host test in `userland/otp/redoubt/tests/system.rs`: a block built without
   `/dev/cons` (write a fixture variant of `session_built` that skips `builder.namespace("/dev/cons")`),
   `Redoubt::new` succeeds, `console_read()` is `Eof`, `console_write` no-op, `console_size` None;
   ideally run a module to exit via `beamlet_redoubt::run` as `fake-redoubt` does. Pages:
   beamlet.md platform table (`/dev/cons` optional), agents.md (output nowhere but /project/out).
3. Rename the exit line to `steward: lease NAME of ACCOUNT/{} exited CODE` (use
   `steward.lease_name(object.id)`; for a plain session the name is `alice/session-2`), add the
   row to steward.md's console-line table (search "audit records are console lines").
4. `cargo +nightly fmt`, `cargo testbench docs`, host tests of beamlet-redoubt (`cargo test -p
   beamlet-redoubt` from `userland/otp`, its own workspace), then `make prebuilt`, then
   `make -k -j4 set CASES="steward-lease-ends steward-end-lease steward-sub-agent-dies-with-agent agent-login shell-compile size-budget ..." WIDTHS=rv64,rv32`
   (split into two jobs under 2 h each; copy transcripts from the newest run dir at once).
   Read the lease-end latency from `steward-end-lease`'s alice transcript (the bench stamps
   each matched step) and quote it.
5. Finish the report (`.wash/local/STEWARD3A-report.md`: head, gates, latency, Q3), amend the
   commit, report with member_update (head and gates first, ≤2000 bytes), set waiting.

## What consumed my context

Reading the core/server/oracle/model/init surfaces (~30 % of it), two design questions and
their rulings, the long tail of test/expectation fixes with their rebuild cycles (each prebuilt
+ case run reported only a FAIL line, so transcripts had to be captured by hand), and the
63 MB `.img` I grepped by mistake once (use the `.log` of a run dir, never `*`).
