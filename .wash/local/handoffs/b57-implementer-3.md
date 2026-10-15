# Handoff from b57-implementer-3 (2026-10-15, checkpoint at 70 % context)

## Branch state

- **B51** (active, mine): worktree `/home/mcloonan/redoubt/.worktrees/B51`, branch `wp-B51` from main 9d680b781, **one commit 03423ecf0**, worktree clean. The commit is the full phase-2a fix (see below), not WIP. Report: `.wash/local/B51-report.md` ('Design', 'Phase 2a'). Assignment 31c3d9e327c88ebb1c026a66805a9735 was resolved by my phase-1 report; later work went as progress messages. Tier A: steward-red-2 reviews once phase 2b is done.
- **B54**: wp-B54 0e4f59084 accepted, in train 25; nothing owed. Worktree `.worktrees/B54`.
- **B53**: wp-B53 2abeb0728, waits only on the owner's approval of `.wash/save.sh`. Worktree `.worktrees/B53`.
- **CONW2** (d9123b273) and **B57** (26e47f1d3): published in train 24; worktrees removed or no longer mine.
- Scratch: `$REDOUBT_TMP/B51/` holds logs only (`build-rv64.log`, `build-rv32.log`); no scratch worktrees of mine exist (`git worktree list` shows none under `.tmp/`).

## Traps

- The env of `.wash/local/launch-env-2026-10-12.md` in every shell (PATH to cargo, BEAMLET_TOOLCHAINS, RUSTSBI_PROTOTYPER*, REDOUBT_TMP); every build/case through `scripts/q` (`q run --cores 4 -- ...`, `make -f scripts/jobs.mk prebuilt|set CASES=...|docs`). Never edit a worktree while prebuilt or a case runs ("tree changed"). Scratch worktrees only under `$REDOUBT_TMP/<PKG>/` (git worktree add --detach), removed when done.
- The Edit tool refuses a file changed by sed/rustfmt since its last Read: re-Read the region first. `rustfmt +nightly --config skip_children=true <file>`; `cargo +nightly fmt` per CONTRIBUTING.
- The size budget (`q run -- cargo testbench --exact --arch rv64 size-budget`) counts code lines per crate; a raise needs `Size budget: <crate>: <reason>` in the raising commit. B51's commit raised libs/rt 3927, libs/fileserver 588, servers/walfsd 856.
- `cargo test -p testbench` (no `--lib`: it is a bin crate). The bench's docs case: `make -f scripts/jobs.mk docs`. doccheck's C2 cannot see macro-generated test names (B54 lesson).
- The fake kernel (`libs/rt/fake`) now shapes a refused `call` as a CallOutcome (status the error, lend returned); `f.refuse(pid, "call", Error::Busy)` works for Blkd tests.
- A consoled end-to-end test must wait for the start-up hold probe (`wait_until`) — pattern for any test reading a server's start-up calls.
- plan_get is expensive; `grep -n -A8 'id = "B51"' .wash/plan.toml` instead.
- Shared machine: timing cases only on the orchestrator's word; a case failing only by time beside a bench is rerun alone, not read as a verdict.

## B51: the finding and the evidence so far

Symptom (train 17, both widths): steward-home-quota step 9 `File.rm("/home/alice/a")` after enospc answered `{:error, :eio}`; the case waited to its 610 s deadline. Alone it passes (~225 s). No surviving failing log; passing logs e.g. `.worktrees/B45/target/testbench/run-193040-1791518768959857335/steward-home-quota-rv64-smp1{,-alice.ssh}.log` (rm matched 114 s after ssh start).

The rm's path: `File.rm` → beamlet `userland/otp/redoubt/src/files.rs` (`io.connect` :560, walk, `Tremove` via the aio hub `libs/client/src/aio.rs`) → walfsd `servers/walfsd/src/server.rs` `remove` (:698) → walfs `libs/walfs/src/ops.rs:201` (log write + sync) → range client `libs/fileserver/src/range.rs` → blkd.

Six eio minters (report 'Design' has the tell-tales):
1. **walfsd's range client** (most likely, by reading; not yet shown by a run): any answer to a call that is not the reply asked for was a `Fault` → volume.rs `Error::Io` → walfs poisons → every later request `corrupt` → files.rs:901 `Name::Corrupt → Eio`; silent (walfsd printed `is_corrupt` only at start). The named refusal: the kernel's `Busy` on a `call` whose sender's group at blkd holds WAIT_CAP=32 messages (docs/kernel/ipc.md:313; `Busy` is in the Call error set, libs/sys/src/error.rs:114). Beside boots blkd's group can fill; alone never. The call waits FOREVER, so slowness alone cannot do it.
2. files.rs busy retry (BEAM11): only an op abandoned while waiting to retry → Eio (:227).
3. `Reply::Lost` (:298) from `Outcome::Ended/Flushed`: walfsd died (then `init: restarted`, which the case forbids → it would fail there) or the hub ended the connection (aio `end` on a send error other than Timeout/Busy; B47's Io release race).
4. `io.connect` refused (:560): any refusal → Eio (B61's TooManyThreads). Fixed in the commit (emfile/enomem).
5. the hub's collect timeout (aio.rs ~531) → `[FAILED,0]` → connection ended → as 3.
6. a Tremove answered by a non-Rremove success (:366): no known path.
walfs `NoSpace` → `no_space` → `enospc`, never eio: excluded.

## The fix built (commit 03423ecf0), and what is untested

- `Blkd::call`: `Busy` → sleep `RETRY_US` (10 ms) and retry, up to `BUSY_WAIT_US` (2 s); past it `Fault` with `Why::Busy`. Every fault notes a `Why` (Busy/Call(err)/Answered(code)/Malformed/Shape/Encode); `Range::fault()` (default None).
- walfsd: `Blocks` shares the reason via `Rc<Cell<Option<Why>>>` (returned in `Mounted::Files { fs, read_only, why }`); `Walfsd::with(what, op)` names the op at all 21 sites; `poison(what)` serves corrupt and says once `walfsd: volume poisoned: <what>: <why>\n` on the console (`Walfsd::new(mounted, labels, console)`; bin finds `/dev/cons`; `redoubt_rt::start::say_on`); `poison_line()` for tests.
- beamlet files.rs:560: connect refusal → `Emfile` (TooManyThreads) / `Enomem` / else Eio.
- Bench: `Step::Within(f64)` (case.rs; ssh.rs `Driver.within`, `limit()`, `step()` wrapper, `pump` message "within the step's own bound"); steward-home-quota has `{ within = 120 }` before `{ expect = '\{:rm, :ok\}' }`. No host test for `within` (the case exercises it).
- Fake kernel: refused `call` shaped as a CallOutcome.
- Pages: docs/servers/walfsd.md "Failure and restart" (line, busy rule, bound, two host tests in status); docs/testbench.md steps example (`within`).
- Gates green: fileserver 10+2 (new `a_busy_call_is_made_again_and_a_fault_says_why`, 2 s), walfsd 31+4 (new `a_poisoned_volume_says_so_once`), testbench 195, consoled 15, beamlet redoubt crate check, `./build --arch rv64/rv32 --programs` rc=0, docs PASS, rustfmt clean, size-budget PASS.
- **Untested**: steward-home-quota itself (both widths) with the fix; the reproduction on the base; whether candidate 1 is really the minter (only reading so far); the `within` step on a real run; the poison line on the machine; blkd under a real `Busy`.

## Phase 2b: reproduction recipe (on the orchestrator's word; machine shared)

1. Base reproduction (to name the minter): a scratch worktree at main 9d680b781 under `$REDOUBT_TMP/B51/base` (`git worktree add --detach`), with ONLY a copy of the case toml carrying the probes (never commit the probes): after `{ send = "{:rm, File.rm(...)}\n" }` and `{ expect = '\{:rm, ' }` add alice steps `{ send = "{:probe, File.stat(\"/home/alice/b\")}\n" }`, `{ expect = '\{:probe, ' }`, `{ send = "{:rm2, File.rm(\"/home/alice/a\")}\n" }`, `{ expect = '\{:rm2, ' }`; in bob's session after `bob-done`: `{ send = "{:bobprobe, File.stat(\"/home/bob/x\")}\n" }`, `{ expect = '\{:bobprobe, ' }`. Readings: (a) on the fix's tree the console line `walfsd: volume poisoned: <op>: <why>` names minter 1 directly; (b) alice's probes both `:eio` = 1 or 3/5, `:ok` = 2/4; (c) bob's probe `:eio` = 1 (shared volume); (d) `init: restarted` on the console = 3 (the case forbids it; read the console log); (e) blkd busy count: not instrumented (the kernel's Busy is the sender's; nothing at blkd counts it) — infer from (a).
2. Load: 2-3 boots beside, through q, e.g. `make -f scripts/jobs.mk set CASES="init-boot beamlet-files launch-system"` in another worktree (train-20 or a scratch prebuilt) while running `make -f scripts/jobs.mk set CASES="steward-home-quota"` in the base scratch (prebuilt first in each). Both widths. The quota case is in the `net` class (lock=net): run it as the `set` target does. Logs: `target/testbench/run-*/steward-home-quota-<arch>-smp1{,-alice.ssh,-bob.ssh}.log` in the worktree that ran it; copy them to `$REDOUBT_TMP/B51/` at once (run dirs get pruned).
3. Then the same beside the same boots on wp-B51 (`.worktrees/B51`, prebuilt), both widths: expect PASS; if the eio still comes, the console line names the real minter and the fix follows it, not the other way round.
4. Record every run (exit code, times, the bytes around `{:rm,`) in `.wash/local/B51-report.md` 'Phase 2b', then report to the orchestrator (progress message; the assignment id is resolved), set waiting; steward-red-2 reviews (Tier A).

## Open questions for the successor

- Is 2 s the right `BUSY_WAIT_US`? Chosen as "a group of 32 block requests drains in far less"; the walfsd page says so. If the reproduction shows longer busy spells, raise it with the page.
- Whether `Why` should be exposed by other `Range` users (littlefsd/erofsd use the same client; their volumes' poisoning is their own and untouched).
