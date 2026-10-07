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

## worst-walk hang on 0795e6b54 (k19-implementer-2, 2026-10-06; stopped at the pool checkpoint)

**Cause: a scheduler livelock after the deadline wake, not the destruction.** Found with gdb on
the failing rv64 image (target/k19-final/worst-walk-rv64-run, QEMU started by hand with
`-gdb tcp::1241`):

- Every sample after '250 holders waited' is in the kernel: `sched::leave` -> `Sched::reconcile`
  -> `Queue::raise_floor` (reads every queued budget's sched_state), `sched::audit_marks`, kmain's
  pick. Trap cause is the timer interrupt from U-mode with sepc = 0x1a7b6, the instruction right
  after init's `ecall` (redoubt_sys::ecall): the thread is preempted before it runs one user
  instruction. A breakpoint on `redoubt::dispatch` saw no system call in 5 minutes.
- TIMER: threads = budgets = NEVER (no timed wait armed), slice[0] = armed[0], which tracks `now`.
- So: kmain's `pick` sets slice_end = now + SLICE_US (1 ms; sched.rs, end of `pick`), and the
  rest of the exit path that is *not* an audit (settle, switch, reconcile and several
  raise_floor walks over ~250 queued budgets, SMP1's `waiting` walk of the queue in `leave`,
  checked kframe reads) costs more than 1 ms of icount guest time (shift=3: ~125k instructions).
  The timer is due at the return; the thread is preempted; the next pick does the same. No user
  progress, ever. `audit()` moves the slice end by its own length, but nothing else does.
- rv32 (the orchestrator's run-1345249) stuck at the same line.
- K19's commits do not touch this path (sched.rs, stride, irq.rs unchanged by K19); the case
  passed before the rebase onto SMP1 (14aceaa63), which added per-exit work (Harts wiring, the
  `waiting` count in `leave`, `elsewhere` in pick, sync_icache, per-hart KernelCell asserts).
  Inference: SMP1 pushed the exit path at 250 queued budgets over the 1 ms slice.
- **Not yet confirmed: whether main 14aceaa63 alone hangs.** A run from a git-archive export
  (/tmp/k19-main, log /tmp/k19-main-rv64.log) had reached '250 holders waited' at 13:36 and was
  killed at the pool checkpoint before the verdict. Next step: rerun it; if it hangs, this is an
  SMP1/R12 defect, not K19's.
- Fix options (design questions for the orchestrator/Architect, kernel/scheduling.md R12; not
  improvised): (a) start the slice at the return to user (set the slice end in `leave`'s to_user
  path), so a slice is always 1 ms of user time; (b) guarantee forward progress, e.g. a pick
  whose slice is already over at the return still runs; (c) make the exit path O(changed) (no
  raise_floor/queued walks per exit). Each changes R12 traces the model/oracle compare.

Model: R10DeliveredMidDestruction, R10ExitNoticesOutlivePayer, R10CreatorDeathSparesProcess run
alone in the release mutations test: rc 0 each, ~1 s. The full suite was not run. The
predecessor's stale release run (3.5 h) was killed. The orchestrator's /tmp/k19-model*.sh debug
runs (pid 1095538 has a thread spinning since about 11:00) are still running and were left alone.

### Confirmed (after the q resume, before the second checkpoint)

- **main 14aceaa63 alone hangs the same way.** Its rv64 worst-walk run (from the git-archive
  export /tmp/k19-main, through `q run --cores 1`) printed '250 holders waited' at 13:54:36. By
  14:11, 17 minutes later, there was no 'one holder destroyed', and I killed it at the checkpoint.
  Without the hang the destruction follows within about a minute. So SMP1 introduced the
  livelock, and K19 only inherits it.
- **The slice is the cause.** K19's head (0795e6b54, exported to /tmp/k19-exp) with
  `slice-10ms` added to worst-walk's kernel_features: PASS worst-walk [rv64, smp=1] 468.6 s, rc 0.
  'one holder destroyed, killed: true' and WORST-WALK DONE. R10 2 destructions, p50/p99/max
  16371/16383/16383 µs, threads' ending 13111/13126/13126; pump p99 1028 µs, expiry p99
  28072 µs. These are the same numbers as the pre-rebase pass.
- redoubt-model release suite: interrupted at the checkpoint inside mutations_are_caught. Every
  test binary before it was ok (7, 8, 1, 17, 3, 5, 10 tests). Not a verdict.

## Rebased over K24 (main 2151b2aa4): head 6986a2c5a (k19-implementer-2, 2026-10-07)

Commits:
- b7d557e0c model: a destruction delivers nothing until its end
- e5344683c kernel: a destruction delivers nothing until its end
- 22fcdf243 kernel: a destruction's kills, frees and PID moves follow the dying subtree
- 6986a2c5a tests, docs: a destruction at full occupancy is within R10's 30 ms

**Hunks resolved.**
- docs/SECURITY.md: K19's R10 row and main's R12 row.
- tests/size-budget.toml: main's side, then recounted at each commit: model 10253; kernel 9200
  (A) and 9292 (B); ipclist 516 (A) and 532 (B).
- tests/budget-table-attack.toml: main's guest-time file plus K19's `debug_assertions` and
  `timeout_secs = 30`.
- docs/SUMMARY.md: the todo entry dropped; main's file-server entry kept.
- docs/kernel/scheduling.md: K19's R12 status prose with main's count (43). In the residuals only
  "A destruction walks every process" is dropped; K24's slice bullet is kept.

**Two problems the rebase exposed, fixed in the owning commits.**
1. `Mutation::ALL` was left at 148 by a clean merge. K19 and K24 each add one mutation, so it
   must be 149; the model did not compile. Folded into the model commit.
2. rv32 worst-walk's expiry walk went over R12's 30 ms: 30,191 µs against main's 29,338 µs.
   - A Part-A-only run gave 30,183 µs, so the cause is Part A.
   - Part A's fourth member kind made `List::page` build a `Page::Endpoint(u32)` from the link
     word, a conversion inside every link read and write of the collect walk and its sort.
   - `#[inline(always)]` did not help (30,306 µs).
   - What did: a page kind of its own for listed endpoints, `Page::Pumped(frame + 1)`, so every
     member's page holds its link word unchanged; only the kernel's `Frames::at` converts it.
   - rv32 expiry is now 29,297 µs (main 29,338); rv64 27,142.
   - Folded into Part A, whose message now says so.

**Pages corrected.** R10 at full occupancy after K24's slice change: p99 17,098 µs on rv64 and
18,207 µs on rv32, threads' ending 13,097 and 13,796 µs. The run's two destructions take 16.4 and
17.1 ms (rv64) and 17.4 and 18.2 ms (rv32). Updated in budgets.md, the R12 status line,
worst-walk.toml's comment and the docs commit's message. The "near empty" comparison now names
the pair without saying which destruction is which, since the trace does not show that.

**Gates.**
- Per commit (four builds, release and checked, rv64 and rv32, 0 warnings; ipclist tests;
  doccheck; `cargo +nightly fmt --check`): all rc 0. Checked at 79834a893, 8abba09e5 and
  9d2e7902c, and again at Part A and the head after each fix.
- On the head tree (fresh prebuilt, through jobs.mk): the 21 cases on both widths, smp-evict
  rv64, size-budget and docs: 47 PASS, 0 FAIL.
- worst-walk at the 1 ms slice: rv64 PASS 663.6 s, rv32 PASS 729.8 s. Run on 7106a644f; the head
  differs only in pages, a toml comment and commit messages.
- Model, on 06bb72397 (the model is unchanged since):
  - `q run --cores 8 -- cargo test -q -p redoubt-model --release -- --skip steward --skip mutations_are_caught`: rc 0;
  - `REDOUBT_MODEL_MUTATIONS=R10 q run --cores 4 -- cargo test -q -p redoubt-model --release --test mutations`: rc 0.

## Rebased onto 678cdb205 (K23 reap, B18, K25, B14, B21): head d07a4309a (2026-10-07)

- **Resolutions.** In order of the passes onto 158bcaa22, f820b6ba3, ba4aabd8b and 678cdb205:
  - mutation.rs: both sides' variants; ALL 154 (main 153 + R10DeliveredMidDestruction), counted
    from the array entries;
  - contracts.rs: rebuilt as main's file plus K19's own hunk (59 lines, identical to the
    original), after a block-union had cut `reap_empties_and_keeps`;
  - model.md and SECURITY.md R10 rows: main's reap entries plus K19's mutation and test;
    SECURITY.md R12 row: main's, with K25's two mutations;
  - budgets.md: the R10 status lists unioned (36); K19's "nothing delivered" paragraphs, then
    main's budget_reap paragraph;
  - scheduling.md: K19's R12 status prose with main's count (45);
  - SUMMARY.md: the todo entry dropped.
- **budget_reap needs no code change.** It calls the same `destroy_subtree(ss, child, Some(pid),
  None)` that `budget_destroy` uses, so it gets K19's deferral, chain walks, and pump drain at the
  end.
- **Range-diff 99eec6be6 -> head is clean.** Only ALL (153 -> 154), the size ceilings and the
  R12 status count differ.
- **Size.**
  - K19 raises only its own crates: model 10325 -> 10331, kernel 9259 -> 9268 (A) -> 9360 (B),
    ipclist 499 -> 516 (A) -> 532 (B).
  - size-budget still fails on libs/wire (3170 against 3153), and main 678cdb205 alone fails the
    same way, so it is main's. The recount script had also raised wire, littlefsd and walfsd in
    the model commit; I took those raises back out.
- **Gates on the rebased head** (code identical to the head):
  - per commit: four builds 0 warnings, ipclist, docs, fmt: rc 0;
  - build-rv64 and build-rv32: rc 0; prebuilt: rc 0;
  - the 21 cases + budget-reap on both widths, smp-evict, docs, model-host-tests (116.9 s):
    49 PASS + docs + model-host-tests PASS.
  - Carried from 99eec6be6 (clean range-diff): worst-walk rv64 713.6 s and rv32 784.1 s (rv32
    expiry 29,320 µs, R10 p99 18,210 µs); model-mutations 2835.6 s, every mutation caught.

## Rebased onto c425bf4d7 (MODEL1): head 806d6785c

- **Conflicts.** Only tests/size-budget.toml (the model ceiling). The range-diff against
  9c299d743: three commits '='; the model commit differs only in its ceiling, model 10509 -> 10515
  (+6, as before). ALL stays 154: MODEL1 added no mutation.
- **MODEL1's batch and K19's rule do not interact.** Both batch paths in the model's tick
  (`run_slices`, `run_queue_slices`) run only when the next event (timeout or budget deadline) is
  past the batch and `to_pump` is empty. A destruction, by call or by deadline, runs at an instant
  outside a batch, and K19's end-of-destruction pump drains `to_pump` before the tick goes on. So a
  batch never spans a destruction or a deferred pump.
- **Gates through q:**
  - `cargo test -p redoubt-model --release --lib`: rc 0 (8 passed, the tick differential
    included);
  - rv64/model-host-tests: PASS 76.4 s;
  - rv64/model-mutations: PASS 32.6 s, fanned; the q log shows 154 jobs, R10DeliveredMidDestruction
    and R12SliceCountsExitWork among them;
  - size-budget: PASS.

## Red round on 806d6785c: OK with notes, two model P2s folded; head 21eda8acf on bdb38430e

1. **Kill order.** The model's `destroy_budget` killed post-order (children first); the kernel's
   walk is pre-order (the top's chain, then `subtree_next`: first child, the newest, then
   siblings). The model now kills in that order. It keeps the post-order only for the
   scheduler's bottom-up teardown, which is the kernel's too.
   - The contract changed with it: K is in B (the top) and reports to E; S is in C, waits on E
     before R, and reports to F.
   - K's notice is then the only one owed on E. The specified outcome (R takes it) holds for any
     kill order, and R10DeliveredMidDestruction is still caught: S, killed after K, takes K's
     notice ("caught by ipc_contracts seed 0: the notice owed on E goes to the receiver that
     survives").
2. **Settle per destruction.** `destroy_budget` ends with `settle()`, as the kernel drains
   `pump_listed` at the end of each `destroy_subtree`. So of two deadlines due in one expiry, the
   second finds the first's deliveries made.

The model commit's message says both; its Size budget line now covers the walk and the settle:
model 10515 -> 10522. docs/kernel/model.md's contract line is order-neutral and unchanged.

**Gates:**
- model lib release (the tick differential): rc 0;
- current_contracts: rc 0 (17);
- model-host-tests: PASS 55.3 s;
- model-mutations: PASS 46.1 s, fanned;
- docs: PASS; size-budget: PASS.
- Range-diff against 806d6785c: the model commit changed; commits 2 and 3 '='; commit 4 differs
  only by main's R13 context row in SECURITY.md.
