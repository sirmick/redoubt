# K19 report

Branch wp-K19 from main a9f54ffcb. Four commits, never pushed:

- 4ce75ef7a model: a destruction delivers nothing until its end (trace, mutation)
- 870ee35f0 kernel: a destruction's kills, frees and PID moves follow the dying subtree (Part B)
- a40aa558e kernel: a destruction delivers nothing until its end (Part A)
- 9735b37b1 tests, docs: a destruction at full occupancy is within R10's 30 ms

Part B landed before Part A: the orchestrator cleared Part B's kernel code while the Architect
ruled on Part A. They touch different functions and can be reordered at the fold if wanted.

## Result

`bench:worst-walk` (by name, final code, checked build, net of audits), R10 at full occupancy
(510 processes, 129,796 threads):

| | p50 / p99 / max (µs) | its threads' teardown (p99) | margin to 30 ms |
| --- | --- | --- | --- |
| rv64 | 16,227 / 16,237 / 16,237 | 12,995 | 13.8 ms |
| rv32 | 17,244 / 17,281 / 17,281 | 13,707 | 12.7 ms |

Before: 53.5 / 58.3 ms. Near empty: 16.1 / 17.1 ms. So the full-fill destruction now costs what
the near-empty one does. Part B alone gave 16,173 / 17,217 µs p99; Part A adds the drain (tens
of µs). The must_fail on R10 is gone. The final run was made with must_fail still present; it
"passed, but must fail" on both widths, so without the line the same build passes. The walks'
own bounds (pump, expiry) held on both runs.

## Checkpoint 1: the trace (refuted)

The suspicion was that a kill's pump delivers a dying-stamped send mid-destruction. The trace
refutes it. Pumps outside a destruction are eager, so at the mark nothing waits that a waiting
receiver could take. A kill makes no survivor newly able to take a message: it adds notices,
wakes callers with Dead, and frees only the dying process's own open-call slots. And step 4
fails every stamped message. What a mid-kill pump did allow was a doomed receiver taking a
notice owed to a survivor.

The model trace is `destruction_delivers_at_its_end`, in ipc_contracts, in the replay set, and
as host:redoubt-model::a_destruction_delivers_at_its_end.

Mutation `R10DeliveredMidDestruction` settles after each kill. It is caught by ipc_contracts
and by trace replay. No R4b "doomed takes nothing" mutation existed (only
R4bDeadServerFakesReply), so nothing retires.

## Part A (as ruled)

- **The list.** ipclist gains a to-pump list:
  - head and tail in K_PUMP (kernel words 2,3; K_TIMED moves to 4);
  - links in endpoint list words 12,13 (ENDPOINT_WORDS 14);
  - membership by its links;
  - a fourth member kind, Endpoint.
- **Listing.** `pump_later` lists inside a destruction and pumps otherwise. Its sites:
  process_ending, settle_notice (pump_endpoint), fail_wait.
- **Unlisting.** The owner walk unlists each dying endpoint.
- **Drain.** `pump_listed` runs after destroy_marked and end_destruction, before the bill,
  inside the R10 bracket. Checked build: each drained endpoint's owner is live and not dying,
  and the list is empty after the drain. `pump` asserts it never runs while deferring. The
  reasoning above is in destroy_subtree's comment at the drain.
- **The five checks**, all deleted:
  1. next_receiver's doomed check;
  2. owed_notice's doomed check;
  3. abandon's dying-owner branch. abandon always owes; endpoint_dying now fails callers
     before dropping notices.
  4. settle_notice's dying-creator branch and allowed's dying-owner branch. Step 3's free
     withdraws the notice from exits, and a dying endpoint drops its own.
  5. budgets_dying's ordering comment.

  process_is_doomed and runs_in_dying are gone.
- **Pages:**
  - R10 step 4 now reads "from the mark on, no message stamped with a dying budget is
    delivered", and a sentence follows step 9 (pumped once, at its end; nothing delivered while
    a budget is dying);
  - R10 residual item 4;
  - R4b's doomed sentence is now a consequence;
  - I15 "Kept in": endpoint_dying, both destruction facts.

## Part B (as ruled)

- **Words.** A charged chain (creator) and a counted chain (counted_in) of process objects:
  - process list words 2-5 (PROCESS_WORDS 6);
  - budget heads CHARGED_WORD 107 and COUNTED_WORD 108 (Page::Budget words 2,3).
- **Linking.** Linked in process_create once the handle is installed: a rollback never links,
  as the comment there says. Unlinked in free_object.
- **The walks:**
  - step 2 walks the dying counted chains for live members, caller last; a checked-build
    assert that a kill frees no next member;
  - step 3 pops the charged chains;
  - step 8 relinks onto the parent's chain and recounts.
- **init.** Root destruction alone walks every live PID: init has no object and runs in root.
- **Audits.** check_lists counts both chains from every object and walks them from their heads.
  enumerate_lists walks both from every budget. The checked build asserts every live PID but
  init has an object.
- **Removed:** find_process.
- **Pages:**
  - objects.md: the chains in "What objects cost", the new words beside the chain heads, and
    the residual;
  - budgets.md residual item 2;
  - model.md: the ipclist line.

## Final commit's pages

- **Removed:** worst-walk's must_fail and its description; the todo page and its links
  (SUMMARY.md, plan/m1-separation.md).
- **budgets.md:** the residual bullet becomes the measured sentence after "the gate's fill".
- **scheduling.md:** R12's status line now gives the destruction's numbers, and its residual
  bullet is gone.
- **testbench.md:** the post_check sentence no longer cites worst-walk's must_fail.
- **SECURITY.md:** the wall residual "walks every kernel-object frame" now reads "follows the
  dying subtree".

## Gates (exact commands through the pool; exit codes)

- **Builds:** `cargo build --profile {release,checked} --target {riscv64gc,riscv32imac}-unknown-none-elf -p redoubt-kernel --features qemu-virt`,
  all four 0, at every commit's tree. `./build --arch rv64` 0. `./build --debug` fails to link
  (FLASH overflow); that profile is not the checked build, and I did not check it at main.
- **Host tests:**
  - `jobserver bounded cargo test -p redoubt-ipclist`: 0 (12 tests);
  - model: current_contracts 0, traces 0;
  - `REDOUBT_MODEL_MUTATIONS=R10DeliveredMidDestruction ... --test mutations`: 0;
  - full model suite (`jobserver bounded cargo test -q -p redoubt-model --release`): see the
    report message;
  - redoubt-kernel has no host tests (test = false).
- **Source gates:**
  - docs checker (`cargo test -q -p redoubt-doccheck --test docs`): 0;
  - size-budget 0, with ceilings kernel 8823 -> 8921 (+91 Part B, +7 Part A), libs/ipclist
    499 -> 526, model 10240 -> 10244;
  - unsafe-budget 0, unchanged;
  - no-cruft 0.
- **formatting case:** not run. The pool's environment reports nightly rustfmt not installed.
  `cargo +nightly fmt --all -- --check` run directly has no diffs.
- **Cases, both widths, final code (make jobs.mk), all rc 0:**
  - own cases: endpoint-destroy-full, endpoint-destroy-open-calls, budget-destroy-kills,
    ending-pumps-once, destroy-keeps-notices, destroy-keeps-notices-creator,
    process-lifecycle, redoubt-dead, sched-destroy-billing, pid-pinning-attack,
    handle-chain-attack, handle-chain-fault, process-chain-fault, budget-deadline, timeouts;
  - smoke: userland-boot, init-boot, bench-net-peer, ipc-outcomes;
  - also: budget, budget-destroy-attack, budget-destroy-growth, deadline-flood-billed,
    redoubt-revoke, process-attack.

  The `budget` filter also ran every budget-* case, and they passed. The `budget` target's
  rc=1 was only size-budget flagging the then-uncommitted raise.
- **worst-walk** by name on both widths, numbers above.

## Summaries checked

- **Updated:**
  - kernel/src/{budget,message,process}.rs module docs (budget.rs said a subtree is found by
    scanning: fixed);
  - libs/ipclist crate doc;
  - budgets.md, objects.md, ipc.md R4b, invariants.md I15, scheduling.md, model.md,
    testbench.md, SECURITY.md (R10 row, wall residual), SUMMARY.md, plan/m1-separation.md.
- **No change needed:** processes.md ("a doomed process's thread takes none" still true, now
  structurally); README.md and GETTING-STARTED.md (no destruction claims).
- **Stale, not mine (IPC3's), left:** SECURITY.md R4b residual "Delivery walks every thread";
  invariants.md I11 "Kept in next_sender ... W_DUE".

## Risks

- Part B commit precedes Part A; reorder at the fold if wanted.
- SMP1 rebase: SMP1 has been sent the words. The SERVED static is unchanged.

## After the report (orchestrator's two asks)

- **Sense fixes outside owned paths** (fixup f18a6db0a, folds into 9735b37b1):
  - SECURITY.md's R4b residual now reads "A server pays for the calls it holds until it replies
    or dies; delivery reads the endpoint's own lists, never every thread".
  - invariants.md I11's Kept in now cites `pick` and redoubt-ipclist's groups: groups ordered by
    when their turns fall due, a take (`served`) moving its group behind the rest in one write,
    and a refused message taking its turn. I checked that `deliver` calls `served` before
    `prepare`.
- **Order at the fold.** Part B does not depend on Part A:
  - Part B's walks, chains and audits use nothing Part A adds.
  - Its step-2 argument (a kill frees at most its victim's own object) held with mid-kill pumps
    too: Part B alone passed every case and both worst-walk widths with that assertion checked.
  - So the fold puts Part A before Part B. The overlaps to resolve are `runs_in_dying`'s doc
    comment (B edits it, A deletes it), destroy_subtree's closure, and the size ceilings, which I
    recompute for each commit.
  - I rebuild and rerun the short checks at each folded commit.

## Editor round (BLOCK), as ruled by the orchestrator

Fixups now, to be folded once with the red's and the simplifier's points.

- **1155eb30d** folds into Part A:
  - budgets.md R10 gains a paragraph after the drain sentence with the kernel fact. Deliveries
    outside a destruction are made at once. A kill gives survivors only notices, callers woken
    with `Dead` and freed open-call slots. Step 4 fails every stamped message. The one thing a
    delivery inside could do is hand a doomed receiver a survivor's notice, which the list and
    its drain prevent.
  - destroy_subtree's comment points at that paragraph.
  - ipc.md R4b gets the editor's sentence.
- **52c332e91** folds into the docs commit: the 53.5/58.3 history sentence is deleted.
- **At the fold:**
  - f18a6db0a is squashed into the docs commit, whose message names the SECURITY R4b and I11
    edits;
  - Part A's message drops ", each added after a finding";
  - commit 1's message reads "a kill's pump delivering..." and "It cannot happen:".

Docs checker 0, fmt 0, checked rv64 build 0.

## Simplifier round (OK with notes): all six P2s taken, as fixups

- **db6ce6cd7 → Part A:**
  - (1) ipclist's crate doc loses "a doomed receiver";
  - (2) pump_later is now pump_endpoint, taking (ss, mm), and the wrapper is gone (3 callers);
  - (3) pump_listed's after-loop assert is gone.
- **d9e28df0e → Part B:**
  - (4) one `processes(owner, head, tail, links)` constructor: exits and reporters `.counted()`,
    charged and counted_in from it;
  - (6) objects.md names the **charged chain** and the **counted chain**, matching budgets.md and
    the code.
- **37bbcd1ce → commit 1:** (5) C is made with the file's `budget()`. B keeps one literal (256
  pages, 4 processes) with a comment saying why.

Checks: ipclist tests 0; checked rv64 and release rv32 kernel builds 0; model current_contracts
and traces 0; R10DeliveredMidDestruction still caught; docs checker 0; fmt 0.

## Red round (BLOCK): one P1, fixed

**P1.** check_lists' new "every live PID but init has an object" assert fired on a
`process_create` rolled back at a full handle table: index_process(child, None) audits mid-call
while the child is still live with no object. Reproduced: budget-table-attack extended to make a
process at a full table, as a checked build, panicked at message.rs:2161 on rv64. Fixed: the
assert exempts a process with no thread yet, which only that rollback leaves without an object,
until the same call ends it. The case passes on both widths and is now the regression (part of
Part B's commit).

The red team's other points were checked OK. Their note on the mutation stands: it catches
because the model kills post-order (C before B), and the kernel's kill order is unspecified;
nothing retires.

## The fold (once, after all three reviews)

wp-K19, base a9f54ffcb:

- 1d7d35b93 model: a destruction delivers nothing until its end
- 2e8455ac9 kernel: a destruction delivers nothing until its end (Part A)
- 3b3a3ffa1 kernel: a destruction's kills, frees and PID moves follow the dying subtree
  (Part B)
- 81a089512 tests, docs: a destruction at full occupancy is within R10's 30 ms

- **Order.** Part A now precedes Part B, since Part B does not depend on it. Part A keeps
  `runs_in_dying`, which the pre-chain kill loop uses, and deletes only `process_is_doomed`;
  Part B deletes `runs_in_dying` and `find_process`.
- **Size ceilings**, recounted at each commit:
  - Part A: kernel 8823 -> 8831, ipclist 499 -> 515;
  - Part B: kernel -> 8923, ipclist -> 531;
  - model 10240 -> 10244 (commit 1).
- **Messages:** the editor's three edits are in; the docs commit's message names the SECURITY
  R4b and I11 corrections.
- **Each folded commit on its own:** the four kernel builds (release and checked, both widths;
  no warnings), ipclist tests, docs checker, size-budget and `cargo +nightly fmt --all --check`,
  all 0.
- **Final tree** = the pre-fold head (k19-prefold, a local ref I will delete) except two ipclist
  tests swapped in order and the recounted ceilings.
