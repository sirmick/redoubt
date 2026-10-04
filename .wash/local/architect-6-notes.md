# architect-6 working notes (for the handoff)

## Rulings so far
| Thread | Ruling | Where |
| --- | --- | --- |
| K16-timer-flood-share (re-rule) | Option (1): K20 goes ahead as a billing fix. The proof is a direct trace/oracle check plus a recorded negative (old billing behind a test-only feature), no padding self-check. Second leak: `leave` starts user time before walk/reconcile/rearm. K16 no longer needs K20. | K20-rerule.md (the old brief points to it) |
| K20-empty-timer-entry | (c) arm the timer only when a thread blocks (`settle`, not `mark`); (b) a wait that ends early pays for its stale walk/entry, found by `next_timeout`; (a) refused. Residual: a budget destroyed before its deadline costs one walk, nobody's. Third timer-flood case "cancelled waits". | K20-empty-timer-ruling.md (page lines replace the brief's) |
| RT1-unsafe (owner's request) | Node RT1 (todo, needs INIT2): 14 -> 10 (hook in entry!, heap words pair, premapped); heap tests into rt-miri | RT1-implementer.md |
| B7-bench-build-race | Node B7 (todo, Tier B, S): pack cargo-reported artifact, per-run dirs; B6 separate, needs B7 | B7-implementer.md |
| GATE1-trace-ring (2) | Diagnosed: alloc_frame first fit (R12 violation). Node K21 (free list, rollback walk, dma pool/run). GATE1 (ii): ring from top of RAM, 16384 pages; memory_mib 288 only if gate fill >75%. | K21-implementer.md |
| GATE1-trace-ring | (a) trace ring 32 -> 64 MiB (PAGES 16384), one size both widths, GATE1 carries it; scheduling.md residual "64 MiB less RAM"; fill reported; (b) refused | thread |
| (INIT2 R33/R34 bench) | One bench refusal each kept: budget-handle, confined-server; other variants host-only | message |
| INIT2-review-pages | Ruled lines all present; 3 content fixes + 2 reflows; INIT3 body +2 items | INIT2-architect-review.md |
| INIT2-page-lines | Confinement bullets in the ruled order; step 2's minting line given | thread; resolved |
| INIT2-failure-status-and-unsafe | (A) `system_reset` kind 3 (SystemFailure), devices.md sentence given; (D) the bundle view in libs/rt (budget 11 -> 12), init at 0 with forbid on the binary | thread; resolved |

## Watch at merges (in addition to architect-5's list)
- **K20:** the direct check fails under the negative feature on both widths; the feature is listed
  in scheduling.md's R23 diagnostic-features sentence; `sched-pad` is absent; leave's tail is
  billed to the payer up to the return; the report records the non-reproduction.
- **INIT2:** the devices.md kind-3 sentence, with its status line naming the 255 refusal cases;
  no refusal case judged on exit 0; rt's budget at 12 with "the bundle's view" in its name;
  init's binary has `#![forbid(unsafe_code)]`; init.md step 2's minting line; the confinement
  bullet order.
