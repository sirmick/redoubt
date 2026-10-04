# Architect handoff (architect-6, 2026-10-02)

Read the earlier handoffs first, newest first: architect-5-handoff.md, architect-4-handoff.md,
architect-2-handoff.md and architect-handoff.md. Their working rules stand:
- pages go on main, staged by path;
- run doccheck before a commit:
  `./.wash/local/in-dev bash -c 'cd /work && cargo run -q -p redoubt-doccheck'`;
- QA bodies are at most 2000 bytes, so detail goes in a file;
- no thread names or dates on pages;
- give exact lines to a package rewriting a page, and check them at its review;
- nothing goes on a page before its code;
- end your turn after a decision_request.

My working notes, a one-table summary, are in `.wash/local/architect-6-notes.md`.

Two habits that served:
- Read the code before ruling. Three of this stretch's rulings turned on a line the question did
  not name: `leave`'s `user_since`, `mark` arming before `settle`, `alloc_frame`'s first fit.
- Messages cross often. When a question arrives that you already answered, reply once,
  pointing at your answer, and adjust only for new numbers.

## Rulings of this stretch, and where each lives

| Thread | Ruling | Where |
| --- | --- | --- |
| K16-timer-flood-share (re-rule) | K20 goes ahead as a billing fix. The proof is direct (trace and oracle), with a recorded negative (`timer-tail-billed`) and no padding self-check. Second leak: `leave` started user time before its walk and reconcile. K16 no longer needs K20. | K20-rerule.md; merged in K20 |
| K20-empty-timer-entry | (c): the timer is armed only when a thread blocks (`settle`, not `mark`). (b): a wait that ends early pays for the interrupt it leaves, found by `next_timeout`. (a) refused. Residual: a budget destroyed before its deadline costs one nobody's walk. Third flood case, "cancelled waits". | K20-empty-timer-ruling.md; merged in K20 (81b5ea38b), pages checked on main |
| INIT2-page-lines | Confinement bullets in kind order; step 2's minting line | resolved; in INIT2 |
| INIT2-failure-status-and-unsafe | (A): `system_reset` kind 3 (SystemFailure), with devices.md's sentence. (D): the bundle view in libs/rt; init has no unsafe. | resolved; in INIT2 |
| INIT2-review-pages | Ruled lines present. Fixes: the watcher clause, the no-keyd sentence, fsd on a dashed edge, budgets.md's INIT_PAGES list, two reflows. INIT3's body gained 2 items. | INIT2-architect-review.md; applied in fix round 1 |
| (INIT2, by message) | R33 and R34 keep one bench refusal each: `init-refuses-budget-handle` and `init-refuses-confined-server`. Their pages name a boot as the verdict. Other variants are host tests only. | message to the orchestrator; INIT2 round 2 |
| RT1-unsafe (the owner's) | Node RT1: libs/rt from 14 to 10 (the panic hook named in `entry!`, the heap's words pair, `premapped`). Heap tests into rt-miri. No bitmap, no `slice_at`. | RT1-implementer.md; node RT1 (todo, needs INIT2) |
| B7-bench-build-race | Node B7: isolate overlapping runs (pack cargo's JSON-reported artifact; per-run dirs `target/testbench/run-<pid>/` and `last`); no lock. B6 stays separate and needs B7. | B7-implementer.md; node B7 (launched) |
| GATE1-trace-ring | The ring is 16384 pages (64 MiB), one size, taken from the **top of RAM** (GATE1 carries it). K20's records are not made conditional. memory_mib = 288 for the gate only if its fill exceeds 75% of the ring (the 288 MiB run decides). Page line: scheduling.md "Measured on QEMU, on one hart" ends "... and 64 MiB less RAM for the budget tree, taken from the top of RAM so the frames below sit where a release kernel's do." | thread; GATE1 |
| (GATE1 diagnosis) | Kernel finding against R12: `alloc_frame` first fit, plus `alloc_contiguous` and `release_owned_frames` scanning RAM. Node K21. | K21-implementer.md; node K21 (todo, M, no needs) |

Awaiting the owner, carried from architect-5: whether to cut **K19 "pumps at the boundary"**
(`.wash/local/destroy-simplify.md`, Tier A, M, after K16). If the owner says yes, write its node
and brief from that file. It rebases on K16's walks.

## Open, and what waits on what

- **GATE1**: held at f0e5ce674, applying the ring ruling (from the top of RAM). Waiting on its
  288 MiB gate run, churn back at 499 on both widths, and the ring's fill at the sweep's worst
  seed. **INIT1** merges right behind it.
- **INIT2**: reviewed and waiting; it needs INIT1. Round 2 restores the two cases (R33, R34).
- **K16**: held at its commits 1 and 4. It needs GATE1 and INIT1. It no longer needs K20, which
  merged.
- **K20**: merged (81b5ea38b), and its page lines were checked.
- **B7**: just launched.
- **RT1**: todo, behind INIT2.
- **K21**: todo, no needs. It touches mem.rs, process.rs and dma.rs. K16 and GATE1 also touch
  kernel files, so whichever merges later rebases. `kernel_frame` must keep taking from the top.
- **INIT3, INIT4**: todo. INIT3's body now holds the console-disconnect case, and the rule that a
  server which exits during the boot falls under the reboot rule.
- **K19**: the owner's call.

## What to watch at the merges

- **GATE1:**
  - `kernel_frame` takes the ring from the top of RAM;
  - trace::PAGES is 16384, and its doc comment says 64 MiB;
  - scheduling.md's residual line is exactly as above;
  - churn passes on both widths;
  - the report gives the ring's fill at the worst seed, below 75%, else memory_mib 288 with the
    reason;
  - architect-5's GATE1 items still stand: the notice met net with two leases, the sweep's worst
    recorded, the per-width seed sentence, and kernel/README.md#containment built.
- **INIT1:** architect-5's items: no program count on boot.md; "Root, system and users" built at
  15 and 47; devices.md's `device_info` built.
- **INIT2:**
  - the five review items as written in INIT2-architect-review.md;
  - devices.md's kind 3 sentence;
  - R33 and R34 status lines each name their one bench case;
  - SECURITY.md's rows follow;
  - rt's unsafe budget at 14 with "the bundle's view" in its name;
  - init's `forbid(unsafe_code)`;
  - architect-5's INIT2 list.
- **K16:**
  - architect-5's items: the counts move with the code, the "(at most `system`'s process
    limit)" wording, GATE1's targets kept;
  - it rebases over K20's `message.rs` lines (`mark`/`settle`/`next_timeout`), keeping them.
- **K21:**
  - scheduling.md's free-list sentence, and memory.md's sentence plus `dma_alloc`'s design;
  - scan-bounds extended, with the `alloc-first-fit` negative failing on both widths;
  - the free-list audit stamped;
  - no RAM scan on a call path remains: check `release_owned_frames`;
  - the report lists every free path that pushes.
- **B7:** testbench.md "How to use it" and GETTING-STARTED.md line 85 as in the brief; host
  tests for interleaved builds; the reproduction's numbers before and after.
- **RT1:**
  - the count reached site by site (target 10);
  - the panic hook form chosen, or the transmute kept with its reason;
  - heap tests in rt-miri's list;
  - the native.md bullet with the real N.

## Fragile pages

- **scheduling.md:** K20's lines, GATE1's ring line and K21's R12 sentence all land here, plus
  Responsiveness's measurement prose.
- **init.md / budgets.md:** INIT2, and later K16 and INIT3.
- **timer.md:** K20's lines are merged; nothing pending.
