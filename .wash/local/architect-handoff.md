# Architect handoff (2026-10-01)

The resident Architect of the Redoubt workspace. You answer design questions on QA threads,
write the rule on its owning page, and cut packages and briefs. You don't write code. Commit
page changes on main, staged by path, and run the docs checker before every commit:
`./dev.sh bash -c 'cd /work && cargo run -q -p redoubt-doccheck'` (silent means clean).
- The checker's rules bite: the first citation of a rule is "R23 (no test channels)", with its
  name; no QA thread names in pages; no dates on pages.
- QA and result bodies are at most 2000 bytes; put detail in a file.
- `decision_request` blocks you until the owner answers.

## Rulings today and their decision refs

| Ref | What |
| --- | --- |
| bad3803c6 | (earlier) R3's notice order is not a rule; `docs/todo/process-ending-pumps-once.md` (K13). |
| 9f0b47f38 | (earlier, MOD1) R2 is least recently served (IPC2). |
| cc6b697f2 | steward.md R37: sessions and agents are numbered per (account, label set). IPC2 finding 2. |
| fba3bc92e | **Owner decided** (IPC2-steward-family): a shared server's own crash is a stated residual of R37, written on steward.md R37 and its residual risks, the TENETS side channels and the SECURITY.md R37 row. P10 replays call-caused crashes and observes unlabelled take order. **Not awaiting the owner any more**; IPC2 deliverables 3 and 4 are unblocked. |
| dd862a390 | **Owner decided** (GATE1-notice-late, option A): the latency targets exclude the checked build's audits, on scheduling.md#responsiveness and testbench.md#checked-builds; a residual bullet on scheduling.md; `docs/todo/latency-excludes-audits.md` (K15). |
| 4a24c6f8b | testbench.md "Starting a case's programs": a tester learns its programs and their budgets from a `programs` data entry. |
| a9eed8300 | Same section: each program's budget is an equal share of `system` at weight 1,000. |
| 39ccb59a0 | devices.md "Which process gets which device": names are matched with `device_info`. |
| 378a67877 | `docs/todo/wall-clock-flakes.md`: `uaf-lent-page`, `touch-beyond-ram` and rt `parked.rs` (probably the B4 package). |
| 8074ecf59 | budgets.md "The tree from the boot manifest": `root` keeps `init`'s boot charges plus `INIT_PAGES` = 1,024, with the reason. |
| 4ace1722d | boot.md planned handoff: a bundle entry named `grants` is data and grants nothing. The loader's refusal goes (option C). |

## Open design state per package

- **IPC2** (active). Both findings are ruled; see fba3bc92e and cc6b697f2.
  - The implementer fixes `model/src/steward.rs`'s counter per (principal, label set), moves P10
    to call-caused crashes, and adds the take-order observation, which must catch `R2OneCursor`.
  - Watch for: whether steward.rs was in its owned paths (I told it to ask the orchestrator).
- **K13** (parked, needs K12 and IPC2: one writer at a time in `message.rs`).
  - Brief: `.wash/local/K13-implementer.md`.
  - Open: remeasuring `process_ending` needs a phase split. K12's "T-record bisect" instrument
    is NOT in the tree or on any branch; the brief says to ask the orchestrator. If asked, rule
    that it re-adds test-only phase records under `sched-trace`, like K15's audit records, never
    in a default build.
- **K14** (active; process_map backs before refusing, and the boot stack reservation). Not my
  brief.
  - Its lines (`setup_loader_process`, `DEFAULT_STACK_SIZE`, the loader's `USER_STACK_*`) are
    also needed by INIT1 deliverable 2. INIT1 waits for K14's merge.
- **K15** (todo, cut by me). Brief: `.wash/local/K15-implementer.md`. The accepted shape:
  - test-only audit begin and end records in `sched.rs` trace, around `check_object_indexes`
    (after Y in `budget.rs` `destroy_subtree`) and `check_process_index` (`process.rs`
    `index_process`);
  - the oracle and post-check subtract the audit time inside each window (deadline notice, the
    wakes, R10 asserted zero, lease end) and report the total;
  - a negative run with an `audit-unstamped` feature must miss;
  - remeasure, drop the residual, delete the todo page.

  Open point: the deadline notice is measured by the program's `time_now`, so the program must
  print each window's ends for the oracle.
- **GATE1** (reported, stopped). It resumes on K15's merge, unchanged. Evidence:
  `.wash/local/GATE1-notice-late-architect.md` §1-7, `GATE1-notice-split.md`,
  `GATE1-wake-after-Y.md`, `GATE1-audit-confirm.md`.
- **INIT1** (active). Brief: `.wash/local/INIT1-implementer.md`. D1 (`device_info`) is built on
  wp-init1 (50f71a55e). Risks:
  - D2: needs K14's merge. `INIT_PAGES` = 1,024: some kernel cases whose first program used all
    of `system` must move their work into a budget carved from `system`; the implementer
    reports which.
  - D2: `BUNDLE_AT` = 0x2000_0000, 512 MiB cap, a memory-layout.md row (approved, on INIT1's
    branch).
  - D2: the `grants` lines to move, listed in my 4ace1722d answer.
  - D2: the "Root, system and users" built section's table, its 63 processes and its "loader's
    programs run in system" paragraph change in the commit that builds the split.
  - D2: devices.md `map_device` (~58) and `device_info`'s "once it is built" (~86-87) must drop
    their stale sentences when `device_info` goes to built.
  - D3: the tester spawns from the bundle's pages and parses `programs`, refusing a bad line.
    Kernel lines that print a PID need patterns in `expect`.
  - D4: `bench-bundle-file` becomes a read-back.
  - Model: `device_info`'s arm in `model/src/kernel.rs` rebases onto IPC2.
- **INIT2** (todo). The manifest, consoled's `[con N]` prefixes, bucket counts, `./mkimage`.
  - It must say how `init` checks its own usage against `INIT_PAGES` and refuses the boot.
  - It owns the `/boot` half of the data entries.
- **B4.** Presumably the package cut from `docs/todo/wall-clock-flakes.md`. Tests-only:
  - the holder answers `SYNC` only after it holds the lend;
  - the grabber and the survivor wait on exit or abandoned notices;
  - `parked.rs` runs on a clock the test drives;
  - 20 runs in a row under load.

  `uaf-lent-page` can currently pass without testing reuse.

## Pages I consider fragile or stale

- **scheduling.md, Responsiveness.** Five sweep tables and their prose:
  - The editor noted the sweep lines read as history: the first sweep's targets (20/115) were
    superseded by the third and fourth (25/95).
  - The fifth sweep's closing paragraph repeats the fourth's worst numbers.
  - The new audit paragraph and residual stay until K15 lands.

  Candidate for a cleanup: keep the current targets with the sweep that set them, and move the
  rest out (to git history) or trim it.
- **devices.md planned sections.** `map_device`'s "the kernel says nothing about it" and the
  `device_info` section's forward note go stale when INIT1 merges. "Which process gets which
  device" and the planned handoff assume INIT2's manifest.
- **boot.md.** The built "The boot bundle", "Devices handed to the first program" and Authority
  sections describe today's loader. The planned section is where the truth goes after INIT1.
- **budgets.md, "Root, system and users"** (built) versus "The tree from the boot manifest"
  (planned): they disagree on purpose until INIT1 D2.
- **ipc.md Residual risks** still lists the R2 single cursor (until IPC2) and the ending
  process's pump (until K13).
- **steward.md R37:** now long. It is planned, and its residual is owner-approved.

## Briefs under `.wash/local/`

- `K13-implementer.md`
- `K15-implementer.md`
- `INIT1-implementer.md`
- `GATE1-notice-late-architect.md`: the full analysis of the late notice.
- `IPC2-implementer.md` was referenced by plan node IPC2 but is not in `.wash/local` on main. It
  may live elsewhere; check before relying on it.
