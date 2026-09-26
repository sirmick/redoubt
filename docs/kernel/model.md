# The executable model

The executable model is a Rust crate, `redoubt-model` in `model/`, that states the kernel's
objects, calls, errors, rules and invariants as code, with the steward's policy on top. Seeded
random sequences of calls drive it, and after every step a checker recomputes each invariant
from the objects themselves. Deliberate breaks of each rule, the **mutations**, show that the
checks catch what they claim to catch. Every run can be written as a text **trace** and
replayed; replaying those traces on the real kernel compares the kernel with the model.

## Purpose

A rule in prose can be read two ways; a rule in code cannot. The model is the design in a form a
machine checks: if any sequence of calls it generates reaches a state that breaks an invariant,
it reports the seed and a shrunk trace that shows how. It is the reference the kernel is built
to: the same call names, the same errors, the same constants and the same order of argument
checks ([ABI](abi.md#errors-and-the-order-of-checks)). A design change is tried in the model
first, because a change that breaks a property fails there in seconds, on a laptop, with the
failing sequence in hand.

## What the model is

Status: built · partly tested: independence from the kernel's source and the empty dependency list are held by `model/Cargo.toml` and `#![forbid(unsafe_code)]`, not attacked by a case · tested: host:redoubt-model::overflow_checks_are_on, host:redoubt-model::map_fixed_near_user_top_does_not_starve_map_anon

- **Independent.** The crate is `no_std` with `alloc`, `#![forbid(unsafe_code)]`, and its
  `[dependencies]` table is empty: it links no kernel crate, not `redoubt-sys`, nothing from
  crates.io. It states its constants itself, with the kernel's names and values: `WORDS` (4),
  `MAX_MSG_HANDLES` (4), `MAX_HANDLES` (4096), `MAX_LEND_PAGES` (16), `MAX_THREADS` (31),
  `WAIT_CAP` (16), `MAX_OPEN_CALLS` (64), `SLICE` (10,000 µs), `STRIDE` (2^20). The one crate
  that uses the model is `redoubt-stride`, as a dev-dependency for its differential test
  ([below](#where-the-model-meets-the-kernels-code)).
- **One method per call.** `Kernel` in `model/src/kernel.rs` has one method for each call, with
  the call's name. Every argument arrives as a raw `u64`, as from user mode, and the checks run
  in the kernel's order: decoding as `redoubt-sys` decodes, then each argument from left to
  right, then permission, then resources.
- **Overflow is a failure.** The root `Cargo.toml` keeps overflow checks on for this crate in
  both dev and release builds, so an arithmetic overflow is a panic, and the runner counts any
  panic as a failure of I14 (no call panics the kernel). `overflow_checks_are_on` fails if a
  build turns them off.

| File | What it holds |
| --- | --- |
| `spec.rs` | constants, errors and the small value types of the calls |
| `syscall.rs` | calls as data, their results, and the other events: user loads, stores and fetches, faults, interrupts, time passing, a thread's record changing validity |
| `kernel.rs` | the kernel's objects and one method per call |
| `sched.rs` | the stride scheduler |
| `ghost.rs` | ghost state: facts the checks need that the kernel does not keep |
| `invariants.rs` | the checker run after every step |
| `check.rs` | the property families, the trace epilogue, shrinking |
| `gen.rs` | the seeded generator of operation sequences |
| `mutation.rs` | the deliberate breaks |
| `trace.rs` | the trace format: writing, reading, replaying on the model |
| `steward.rs`, `policy.rs` | the steward's policy and its properties |

### What it abstracts

- **Memory** is page frames with one word of content each. Loads, stores and fetches check the
  mapping's permissions. A process's own mapping, a server's view of a lend and the lender's
  reservation are separate states. Page tables follow Sv39's layout as a placement rule, even
  when the cost table is rv32's.
- **Threads** have no registers. A call is instantaneous and atomic; time is logical and
  advances only on a `tick`. So the model tests accounting and state changes, not real-time
  latency, and not races between harts.
- **Records** (the user memory a call reads and writes) are abstracted as a whole: owned,
  unmapped, read-only, borrowed, device memory, a copy that faults, or an address checked
  against the modelled mappings. Byte layouts are not modelled.
- **PIDs** are drawn by a seeded generator from 2 to `0xffff`; `init` is 1. This tests reuse
  and accounting, not unpredictability.
- **Addresses the kernel chooses.** A `map_anon` lands above everything the process has
  mapped, from `KERNEL_CHOSEN_BASE` (`0x10_0000_0000`). If that does not fit below `USER_TOP`
  (2^38), it takes the first free gap in between. So the model refuses no placement the kernel
  accepts, even after a `map_fixed` has put a mapping high ([memory](memory.md)). The kernel
  searches a bounded window instead; see Residual risks.
- **Costs** come from a `costs` table: by default the rv64 table (two saved-context pages per
  process, one page for each other object, 128 handles per handle-table page). The property
  tests use 8 handles per page, so handle-table growth and its charge show up in short runs.
- **Boot.** `root`, `system` and `users` get the kernel's weight split: `root` 1,000,000,
  `system` 250,000, `users` 749,000, so `root` keeps 1,000 free for `init`. Page and process
  limits are small fixed numbers (`root`: 1,024 pages, 24 processes), not derived from RAM. The
  default boot has seven device objects: an MMIO range without DMA, an MMIO range with DMA, IRQ
  lines 10 and 11, the reset device, a DMA device whose first reset is not confirmed, and a
  second DMA device whose resets always confirm.
- Where the design leaves a detail open, the model makes a choice and the code says so at the
  site.

## Property families

Status: built · tested: host:redoubt-model::kernel_sequences, host:redoubt-model::budget_lifecycles, host:redoubt-model::scheduler_fairness, host:redoubt-model::flood, host:redoubt-model::every_call_and_error_is_reached, host:redoubt-model::reset_at_one_death_does_not_cover_a_co_holder

A family is a function of one seed and an optional mutation. The runner
(`model/tests/common/mod.rs`) runs seeds on every core, keeps the lowest failing seed, and
counts a panic as an I14 failure. A failing kernel sequence is shrunk to the steps that matter
and printed as a trace. Rerunning the seed reproduces the failure exactly.

The generator (`model/src/gen.rs`) first builds a world worth attacking: principals' budgets
under `users`, some labelled and with accounts, a system budget, shared endpoints, handles
minted into the principals' budgets, and a process in each budget. Then it picks operations
from the model's state: a handle the actor holds, a page it has mapped, a message it serves.
About one argument in ten is hostile instead (a handle index the actor lacks, an unaligned
address, a list over its cap). `every_call_and_error_is_reached` checks over 5,000 seeds that
every call succeeds at least once, every error is returned by some call, and lends, transfers,
each exit cause, interrupts and replies all happen, so no property holds only because nothing
reached it.

| Test | Default sequences | What must hold |
| --- | --- | --- |
| `kernel_sequences` | 20,000, up to 150 steps each | every check of the checker, after every step |
| `budget_lifecycles` | 20,000 | I10 (create-destroy leaves the parent unchanged): a child created, used only from inside its subtree and destroyed leaves every other budget's counters as they were |
| `scheduler_fairness` | 20,000 | R12 (scheduling), as the scenarios below |
| `steward_policy`, `steward_noninterference` | 20,000 and 10,000 | [the steward model](#the-steward-model) |
| `flood` | 20 scenarios of up to 10,000 senders | R2 (fair waiting), I11 (fair turns) and R4a (open calls) under a flood |

A default run is 90,020 sequences; the bench case `model-host-tests` runs it.
`REDOUBT_MODEL_SEQUENCES` sets the count per family. The test `million` runs 1,000,000
sequences in each of the five non-flood families and 1,000 floods; it is marked ignored and runs
only when asked for.

### The checker

After every step, `Checker::check` (`model/src/invariants.rs`) runs its checks in a fixed order:
- the objects' structure and serving state;
- handles: I1 (handles name live objects), I2 (revocation is complete),
  I3 (minted badges are non-zero and narrow) and I4 (only badge-0 handles receive);
- charging: I5 (usage within limits);
- budgets: I6 (labels only grow downward) and I8 (class and account inherited);
- the step's flows and call outcomes;
- memory: I9 (pages W^X, zeroed, lends unmapped) and I16 (DMA pages reset before reuse);
- queued messages, then abandoned-call notices: I15 (abandoned calls reported once);
- delivery, exit notices owed, and timeouts: I13 (every blocking call returns by its timeout).

I12 (ids never reused) is checked from ghost records as ids are issued.

Each check recomputes what should be true from the primary objects (handle tables, budgets,
frames, each thread's wait state) and from **ghost state**, never from the counters and derived
fields the kernel model keeps. Ghost state (`model/src/ghost.rs`) is recorded from the primary
objects at the moment of each event, and nothing in the kernel model reads it. So a bug, or a
mutation, cannot hide itself by also changing what the check believes.

The flow checks judge each step against the ghost records:
- a delivered message is the one sent: its kind, badge, account, labels and handles are what
  the sender's handles and budget gave, a handle revoked meanwhile arriving as 0 and every other
  keeping its stamp;
- it reached a thread receiving through a badge-0 handle to its endpoint, and between user
  budgets only equal label sets flow, the receiver being the endpoint's owner (R1 (flow),
  I7 (every flow obeys R1));
- `LabelDenied` and `Busy` happen only when R1 and R2 say so;
- a caller gets a reply only when its server replied, and an abandoned-call notice goes to the
  thread holding the call, once;
- every completed `call` reports the lend disposition its history implies, and a `present`
  reply only with success or `OutOfMemory`.

For I16, the ghost **arms** each DMA frame against every device its holder could reach, and
disarms a device only on a reset that the device object confirms (not on what the kernel
model reports). No frame in the free pool may be armed. A confirmed reset at one process's death
disarms only that process's frames: a live co-holder that still reaches the device can program
it again, so its frames stay armed until its own death resets the device. A quarantined frame
must be mapped nowhere, and a quarantined device used again is a violation. The default boot's
second healthy DMA device exists so the generator can build the co-holder shape;
`reset_at_one_death_does_not_cover_a_co_holder` builds it by hand.

### Scheduler scenarios

`scheduler_fairness` draws one scenario per seed and drives the scheduler alone through time:

| Scenario | Property |
| --- | --- |
| classic | spinners and sleepers: a spinner gets its weight's share over every interval it was runnable |
| gaming | short bursts, below a slice or at a large weight, buy no more than the weight |
| idle gap | a sleeper waking into an empty queue banks no credit |
| exit churn | exiting, faulting or being killed on the CPU is charged |
| budget churn | creating, running and destroying children (blocking, with a spinning parent, by deadline, parked) gains nothing |
| carve inflation | carving moves share to the child and never duplicates it |
| debt lift | a light grandchild's work reaches a shared parent, normalized by weight |
| idempotence | creating and destroying a child with no run leaves the parent's pass unchanged and its remainder unchanged or smaller by rounding (exactly 0 if it was 0), because the carve rescales the remainder; a carve under a running parent first charges the pending runtime at the weight it ran at |
| rank | every pick among equal passes follows the rank rules, checked by an independent oracle; a wake never preempts |
| shell | a parent that gives each short command a heavy child keeps its share |

The rules themselves are on [scheduling](scheduling.md).

### The flood

`flood` is the endpoint-flooding attack, in the model. A system server receives on one endpoint
with two threads. Bob's processes, with two label sets under one account (so two R2 groups),
run up to 31 threads each, all calling with no timeout. Up to eight other accounts queue
`WAIT_CAP` calls each. Alice makes one call in the middle of the flood. Most of Bob's calls
must get `Busy`, no crowd process may get `Busy` within its own group's cap, and Alice's call
must be taken within as many receives as there are groups. In half the seeds the server
hoards: it receives without replying until the process holds `MAX_OPEN_CALLS`, spread over both
threads, and it must still take a send. The checker runs every 512 steps and at the end.

## Scripted contracts

Status: built · tested: host:redoubt-model::ipc_completion_table_and_rollback, host:redoubt-model::sparse_committed_reply_mask_is_positional, host:redoubt-model::scheduler_contracts_hold, host:redoubt-model::pid_reuse_only_after_notice_receipt, host:redoubt-model::deaf_device_quarantines_the_co_holder_too, host:redoubt-model::partial_overlap_is_refused_whole

Some cases are rare in random runs, so fixed sequences pin them. The IPC contracts are written
from the completion table, not from the model's output. They cover:
- every row of the completion table and the rollback of a reply that cannot be written, for
  R13 (one outcome per call); reply masks that are positional, not a count
  (`model/tests/current_contracts.rs`);
- a timeout wakes without preempting, a timeout and a budget deadline at one instant expire in
  that order, and budgets with no free weight are refused a process;
- a PID is reused only after its exit notice is received or dropped;
- DMA frames stay held through `unmap`, return to the pool only after a confirmed reset, and are
  quarantined, with the co-holder's, when a reset is not confirmed
  (`model/tests/dma_contracts.rs`);
- `map_fixed`'s ranges, flags, overlap and cost checks (`model/tests/map_fixed_contracts.rs`).

The mutation check runs the IPC and scheduling contracts first, for every variant.

## Mutations

Status: built · tested: host:redoubt-model::every_rule_has_a_mutation, host:redoubt-model::mutations_are_caught

A **mutation** is one deliberate break planted in the model. Each variant of `enum Mutation`
(`model/src/mutation.rs`) breaks one rule at one place in the model, marked
`self.broken(Mutation::...)`; with no mutation, the model is the specified kernel.
`Mutation::ALL` lists all 127 variants. `Mutation::rule()` names what each one breaks, but not
always by the IDs these pages use. It returns `R1` to `R12` for the variants named after those
rules, folding the open-call and dead-server variants into their parent rule, `I13` for one,
and `policy` for the steward's. The rest return
the name of the design text or the call they were written from: `IPC` for the R13 variants,
`Messages`, `Process`, `Budget`, `Handle`, `serve`, `budget_create`, `process_create`, `mint`,
and labels that name no page at all for the open-call limit, a weightless budget and I16's
variants. The table below maps every variant to its ID; the code's grouping matters only to
the tests that key on it.

- `every_rule_has_a_mutation` requires at least one variant for every kernel rule numbered 1
  to 12.
- `mutations_are_caught` plants each variant in turn. It runs the scripted IPC and scheduling
  contracts, then every property family, the one that pressures the rule first
  (`scheduler_fairness` for R12, the steward families for `policy`, `flood` for the open-call
  limit), up to 20,000 seeds each (20 for the flood). It fails if any variant survives, and
  prints the property and seed that caught each one. `REDOUBT_MODEL_MUTATIONS` narrows the run
  to matching variants, for diagnosis only.

| ID | Variants | What they break |
| --- | --- | --- |
| [R1 (flow)](ipc.md#r1-flow) | `R1SkipLabelCheck`, `R1ExitNoticeIgnoresLabels`, `R1UsageIgnoresLabels`, `R1UsageExemptBySystemTarget`, `R1ExitExemptBySystemExiting`, `R1ChecksReceiverNotOwner`, `R1SenderClassFromStamp` | the label check on messages, exit notices and usage reads, and which side's class exempts it |
| [R2 (fair waiting)](ipc.md#r2-fair-waiting) | `R2FifoAcrossAccounts`, `R2NoWaitCap`, `R2KeyByAccountOnly`, `R2KeyByStampLabels`, `R2SystemCallersShareGroup` | turns, the cap, and how groups are keyed |
| [R3 (lends and abandoned calls)](ipc.md#r3-lends-and-abandoned-calls) | `R3UnmapAbandonedLend`, `R3ChargeStaysWithCaller`, `AbandonNoticeMissing`, `AbandonNoticeRepeated` | an abandoned lend's mapping and charge; the notice, once |
| [R4 (delivery)](ipc.md#r4-delivery) | `R4IgnoreMaxTransfer`, `R4OverdrawOnDelivery` | `max_transfer`; paying for a delivery |
| [R4a (open calls)](ipc.md#r4a-open-calls) | `R4aOpenCallsPerThread`, `R4aFullTakesNothing`, `OpenCallsUnlimited`, `ReceiveDropsOpenCalls` | the limit, per process; sends and notices at the limit; keeping open calls across a `receive` |
| [R4b (a server dies)](ipc.md#r4b-a-server-dies) | `R4bDeadServerFakesReply` | `Dead` for a dead server's callers |
| [R5 (interrupts)](devices.md#r5-interrupts) | `R5NoMaskOnFire`, `R5NoUnmaskOnReceive` | masking on fire, unmasking on `receive` |
| [R6 (charging)](budgets.md#r6-charging) | `R6ChargeAncestors`, `R6OwnPageChargedToItself`, `R6EndpointsFree`, `R6PageTablesFree`, `R6OpenCallsFree`, `R6ProcessObjectFree`, `R6ProcessObjectChargedToBudget`, `R6LendChargedOnce` | who pays for each object |
| [R7 (carving)](budgets.md#r7-carving) | `R7NoCarveCheck`, `R7CarveToZeroFree`, `ProcessInWeightlessBudget` | carving within free limits; no process in a budget with free weight 0 |
| [R8 (accounts)](budgets.md#r8-accounts) | `R8AccountFromArgument` | inheriting the parent's account |
| [R9 (stamps)](objects.md#r9-stamps) | `R9ReceivedHandleRestamped`, `R9MintStampsCaller`, `R9MsgStampIsSenderBudget` | which budget a handle is stamped with |
| [R10 (destruction)](budgets.md#r10-destruction) | `R10KeepForeignHandles`, `R10KeepCarvedLimits`, `R10SpareDescendantProcesses`, `R10ExitNoticesOutlivePayer`, `R10RevokedMessageDelivered`, `R10RevokedCallAnswered`, `R10SweptHandlesDropped`, `R10CreatorDeathSparesProcess`, `BudgetDeadlineIgnored` | everything a destruction reaches, and a deadline destroying the budget |
| [R11 (memory)](memory.md#r11-memory) | `R11NoZeroing`, `R11SetFlagsAllowsWx`, `R11AllowsWriteOnly`, `R11LendStaysMapped`, `R11MapFixedSkipsOverlap` | zeroing, W^X, write without read, lends unmapped, `map_fixed` never replacing |
| [R12 (scheduling)](scheduling.md#r12-scheduling) | `R12PriorityById`, `R12IgnoreWeight`, `R12WakeBanksCredit`, `R12TieQueuedFirst`, `R12RequeueAhead`, `R12RequeueLifo`, `R12PreemptOnWake`, `R12TimeoutWakePreempts`, `R12NoFloorWhenIdle`, `R12ShortRunsFree`, `R12DropRemainder`, `R12ExitRunsFree`, `R12DestroyDropsDebt`, `R12CreateAtFloorOnly`, `R12LiftByMax`, `R12StrideWeightIsLimit`, `R12UnnormalizedLift`, `R12LiftCountsEntryWait`, `R12FoldAtNewWeight`, `R12NoMinimumCharge` | one flat queue, charging, the floor, ranks, preemption, inheritance at create and destroy |
| [R13 (one outcome per call)](ipc.md#r13-one-outcome-per-call) | `IpcWrongLend`, `IpcDropPartial`, `IpcFalseDelivery`, `IpcSkipOutputCheck`, `IpcLeakRollback` | the lend disposition, a partial reply, `delivered`, the completion-time record check, rollback |
| [R14 (unforgeable sender)](ipc.md#r14-unforgeable-sender) | `MsgNoLabels`, `MsgBadgeZero`, `MsgAccountZero`, `MsgIdsGlobal` | the attached labels, badge and account; message ids per receiving process |
| [R21 (crash blame)](processes.md#r21-crash-blame) | `BlameNobody`, `BlameNewestCall`, `ExitWithOpenCallsNotFaulted`, `CurrentNeverSet`, `ReceiveKeepsCurrent`, `ServeIgnored`, `ExitEndpointBadged`, `ExitNoticeDroppedIfNoReceiver` | blaming the current call's sender; how `receive` and `serve` set the current call; a badge-0 exit endpoint; a notice kept until received |
| [I3 (minted badges are non-zero and narrow)](invariants.md#i3-minted-badges-are-non-zero-and-narrow) | `MintFromUnservedMessage` | minting only from an open call of the caller's own thread |
| [I4 (only badge-0 handles receive)](invariants.md#i4-only-badge-0-handles-receive) | `ReceiveWithBadgedHandle` | `receive` through a minted handle |
| [I6 (labels only grow downward)](invariants.md#i6-labels-only-grow-downward) | `LabelsAddedByParentClass` | adding labels needs a system-class caller |
| [I8 (class and account inherited)](invariants.md#i8-class-and-account-inherited) | `ClassNotInherited` | a child's class is its parent's |
| [I13 (every blocking call returns by its timeout)](invariants.md#i13-every-blocking-call-returns-by-its-timeout) | `TimeoutIgnoredWhileOthersRun`, `ExpireBudgetsFirst` | timeouts expire while others run; at one instant, timeouts before deadlines |
| [I16 (DMA pages reset before reuse)](invariants.md#i16-dma-pages-reset-before-reuse) | `K5bFreeBeforeReset`, `K5bQuarantinedSlotCountsAsReset`, `K5bUnmapFreesDma`, `K5bQuarantineChargeDropped`, `K5bQuarantinedDeviceUsable`, `K5bResetClearsCoHolderReach` | pooling only after a confirmed reset, co-holders included; `unmap` keeping DMA frames; quarantine's charge and sweep |
| `policy` | `PolicyVaultWithoutOwnership`, `PolicyApproveIgnoresHash`, `PolicyShowLabelledToAll`, `PolicyNoPendingCap`, `PolicyCapPerAccount`, `PolicyNoFairShare`, `PolicyEndLeaseAdmitted`, `PolicyDeclassifyLive`, `PolicyDeclassifyWithoutReader`, `PolicyBlameNoWindow`, `PolicyBlamePerAccount`, `PolicyNoLockout`, `PolicySequentialIds`, `PolicyLoginWithKeydKey`, `PolicySubAgentOutlivesAgent`, `PolicyUnboundedLease`, `PolicyDeadSessionRequestsKept`, `PolicyRenderNotWhitelisted`, `PolicyLabelledFreeTextShown`, `PolicyWriteUp`, `PolicyServerHoldsSystemBudget`, `PolicyNarrowToSessionBudget`, `PolicyCarveFromUnlabelled`, `PolicyAuditUnfiltered` | the steward model's properties |

Five rules are outside the model and have no variant: R15 (verified boot), R16 (image confinement), R17 (fail closed), R19 (kernel W^X) and R23 (no test channels). The model has no
loader, no bundle, no kernel mappings of its own and no test build. Three rules are in the model
but have no variant: R18 (device authority), R20 (PID reuse) and R22 (range cost). Scripted
tests cover R20 (`pid_reuse_only_after_notice_receipt`) and part of R22
(`huge_len_is_refused_promptly`).

## Traces

Status: built · tested: host:redoubt-model::traces_round_trip, host:redoubt-model::a_rule_breaking_kernel_fails_replay, host:redoubt-model::the_example_trace_is_what_the_model_does, host:redoubt-model::hostile_traces_are_refused_cleanly

A trace is a run as ASCII text: the boot, then each event with the result the model gave.
`trace::record` writes one, `trace::parse` reads it back, and `trace::check` replays it on the
model and requires every line to match. The module is `no_std` and works on `&str`, so a
replayer running on the kernel can use `trace::tokens` as it is.

| Line | Meaning |
| --- | --- |
| `redoubt-model-trace 2` | the header; any other version is refused |
| `boot`, `costs`, `device` | the boot budgets' limits, the cost table, and each device object in `init`'s handle order |
| `start p:1 t:1` | `init`'s first thread |
| `do p:P t:T <call> <arguments> -> <result>` | a call and its result, or `blocked` |
| `read`, `write`, `exec` | a user load, store or fetch of one word: `ok` or `fault` |
| `fault`, `irq N`, `tick DT` | a thread faults; a line is raised; time passes |
| `record p:P t:T <state>` | a thread's record changes validity, as when another thread of its process remaps it |
| `note` | a process or thread the kernel created, naming its PID or TID |
| `wake p:P t:T -> <result>` | a blocked call returns |

**Names** mark the values the kernel chooses, so a replayer can bind each to its own: `p:` a
PID, `t:` a TID, `h:` a handle index, `m:` a message id, `a:BASE+OFF` an address `OFF` bytes
into a region the kernel placed, `pa:` a physical address, `tm:` a time read (compared only for
order). A call's result carries the three facts of R13: `call status=... lend=none|returned|consumed
reply=absent|present`, with the reply's words and handles only when present. A reply's result is
`ok reply delivery=delivered|discarded mask=N`.

Results alone miss a kernel whose state has diverged but not yet shown it, such as a handle that
should have been revoked but is never used again. So random traces end with an **epilogue**
(`check::epilogue`): time passes until every finite timeout has returned; each process with a
runnable thread reads one word of every readable page it maps and the usage of every budget it
holds; `init` destroys the budgets it made, one at a time, reading usage after each; and each
live process closes every handle index from 1 to `EPILOGUE_HANDLES` (64). A thread blocked for
ever, a budget's account (it travels only in messages) and an IRQ source's mask stay unseen.

`model/traces/lender-dies-mid-call.trace` is the checked-in example: a client lends two pages to
a system server, and `init` destroys the client's budget while the server holds them (R3). Its
end:

```
do p:61415 t:3 call h:1 [1,2,3,4] [] a:0x1000000000+0x0@2 forever -> blocked
wake p:38622 t:2 -> ok message call m:1 badge=5 account=1001 labels=[] words=[1,2,3,4] handles=[] buffer=[lend,a:0x1000000000,2]
read p:38622 t:2 a:0x1000000000+0x0 -> ok word 42
write p:38622 t:2 a:0x1000000000+0x0 99 -> ok
do p:1 t:1 budget_usage h:13 -> ok usage [32,10,1,1,50,0]
do p:1 t:1 budget_destroy h:12 -> ok
read p:38622 t:2 a:0x1000000000+0x0 -> ok word 99
do p:38622 t:2 receive h:1 0 0 -> ok abandoned m:1
do p:38622 t:2 reply m:1 [0,0,0,0] [] -> ok reply delivery=discarded mask=0
do p:1 t:1 budget_usage h:13 -> ok usage [32,5,1,1,50,0]
do p:38622 t:2 receive h:1 0 0 -> ok exit p:61415 cause=killed code=0 blamed=0 blamed_labels=[]
```
*Figure: the end of the example trace. The server keeps the lent pages after the lender's budget
is destroyed, is told the call was abandoned, and frees them by replying.*

The tests:
- `traces_round_trip` writes 3,000 random sequences, reads each back to the same operations and
  replays it.
- `a_rule_breaking_kernel_fails_replay` lets a mutated model stand in for a kernel that breaks a
  rule. It replays 2,000 random traces with epilogues, plus four scripted ones (a partial reply,
  current-call blame, the equal-instant expiry order, a quarantined device named again), and
  requires some trace to fail for every variant that results can show. Not required, because
  results cannot show them: the `policy` variants; R12's, since scheduling shows only in timing;
  `R5NoMaskOnFire`, since every result is the same and the model's own R5 check catches it; the
  three open-call-limit variants, since random traces never reach the limit (the flood does);
  and `K5bResetClearsCoHolderReach`, which only the I16 ghost check sees.
- `the_example_trace_is_what_the_model_does` rebuilds the example and requires the same text.
- `hostile_traces_are_refused_cleanly` feeds fixed worst cases and 500 randomly damaged traces
  to the replayer; each must give an error, never a panic.

```mermaid
flowchart LR
    S[seed] --> G[generator]
    G --> K[kernel model:<br/>one step]
    K --> C[checker:<br/>invariants, ghost]
    C -- fails --> X[shrink and<br/>print the trace]
    K --> R[trace::record]
    R --> T[trace text]
    T --> P[trace::check:<br/>replay on the model]
    T -.-> Q[replayer on the<br/>real kernel]
    Q -.-> V[every line<br/>matches]
```
*Figure: how a run becomes a trace and where it is replayed. Solid parts are built; dashed parts
are planned.*

## The steward model

Status: built · tested: host:redoubt-model::steward_policy, host:redoubt-model::steward_noninterference, host:redoubt-model::random_lineage_sequences_and_deliberate_rule_break, host:redoubt-model::audit_authority_binds_purpose_signer_domain_length_and_every_byte, host:redoubt-model::confined_read_down_and_owner_approved_one_item_snapshot_push, host:redoubt-model::approve_and_deny_authenticate_direct_channels_before_any_effect

`model/src/steward.rs` is the steward's policy as a layer on the kernel model, and
`model/src/policy.rs` holds its properties. `init` creates the shared server's endpoint and
starts the steward in `system`, with handles to `users`, `system` and that endpoint. The steward
starts the **server**, a system-class process standing in for `fsd` that holds no budget handle,
receiving on the endpoint. Every principal's budget, its fixed sub-budget per label set, each
session and each agent's lease is a `budget_create`. Each session has a process in its budget
holding its own connection to the server, narrowed to a revocation scope inside the session.
Sessions' work is real calls to the server, and crash blame comes from the kernel's exit
notices of the server. So every kernel check runs under everything the policy does. The policy
is described on [the steward](../servers/steward.md).

| Property | What must hold |
| --- | --- |
| sessions | a session's budget is carved from its principal's sub-budget for its label set (a sub-agent's from its agent's), with the principal's account; its labels are none, or one label the principal owns |
| login | a login used one of the principal's login keys, never one `keyd` holds |
| approvals | an approval came over `approve@box` with the approver's approval key, named the frozen content's hash, and granted no label the approver lacks |
| screens | an approver sees only its own requests, and labelled ones only if it owns every label; rendered text is printable ASCII with capped free text; a labelled request shows none of its free text |
| pending cap | at most `PENDING_CAP` (4) pending requests per account and label set, all from live sessions; a session holds at most its fair share |
| declassification | what is copied out is exactly the snapshot taken at submission, read through a reader budget with exactly the item's label |
| crash blame | an account and label set's sessions are logged out exactly when three server crashes blamed on it fall within ten minutes; no other session is touched; none of its sessions starts for the next ten minutes |
| labelled sessions | a labelled session starts nothing; it only submits requests |
| leases | an agent's budget has a deadline at most `MAX_LEASE` (24 hours) away; a sub-agent sits in its agent's budget and ends no later; an expired lease is gone |
| non-interference | a vault session's work changes nothing an unlabelled session observes: its results, its requests' ids, the usage of `users` and of every principal's budget and unlabelled sub-budget, and the audit records an unlabelled reader may read |
| writes | every write to an item is by a session with exactly the item's labels |
| system budgets | only `init` and the steward hold a handle to a system-class budget; a session's connection is narrowed to a revocation scope inside the session |
| leases end | a lease's sponsor can always end it |

`steward_policy` runs 20 to 160 random policy operations per seed and checks the properties and
the kernel's checks after each. `steward_noninterference` runs one sequence twice, the second
time without the vault sessions' work (their item writes, requests and calls to the server), and
compares everything an unlabelled session observes. The other host tests pin connection lineage
(a delegated badge shares its root's pending share, checked by an oracle that walks parent edges
itself, with a deliberate break it must catch), audit signatures bound to purpose, signer,
domain, length and every byte, a confined session refused a read of lower data while its owner
pushes one approved item up, and `approve` and `deny` authenticating their channel before any
effect.

What the steward model leaves out: SSH (a login is "this key for this user name"), the approval
terminal (a channel is "a connection that authenticated with this key"), and cryptography. Ids
come from a keyed mixer and content hashes from FNV-1a, where the steward uses a CSPRNG and a
cryptographic hash; audit signatures are ideal tokens, and no chaining, truncation detection or
ordering is claimed. Policy numbers are constants: `PENDING_CAP` (4), `DECLASSIFY_MAX` (256
bytes of printable ASCII), `FIELD_CAP` (64 characters), `MAX_LEASE` (24 hours), and the blame
window (3 crashes in 10 minutes).

## Where the model meets the kernel's code

Status: built · tested: host:redoubt-stride::the_crate_and_the_model_agree, host:redoubt-stride::a_broken_model_disagrees

`redoubt-stride` (`libs/stride`) holds the stride arithmetic and ranks that the kernel links. Its
differential test drives the crate, wired as the kernel's `sched.rs` calls it, and the model's
`Scheduler` through the same random sequences: budget creations and destructions (a leaf whose
threads blocked first, the budget on the CPU, a whole subtree bottom-up), wakes, blocks, runs,
slice ends and preemptions. Over 3,000 seeds every pass, entry, remainder, tie, queue
membership, the floor, the tie counters and the running thread must agree after every step.
`a_broken_model_disagrees` shows the comparison bites: with any of 18 of the 20 R12 variants
planted in the model, some sequence disagrees; the list leaves out `R12TimeoutWakePreempts`
(a site in the kernel model, not the scheduler) and `R12ExitRunsFree`. The bench case `stride-host-tests` runs both tests. This compares one kernel crate with the
model, on the host; the kernel's own use of it runs in boot cases
([scheduling](scheduling.md)).

## Replaying traces on the real kernel

Status: planned · M1 (separation and containment)

A bench case boots the real kernel with a replayer program and a set of model traces as files
in the bundle ([test bench](../testbench.md)). The replayer reads each trace with
`trace::tokens`, binds each name to the value the kernel gives, makes each call from the process
and thread the line names, and compares every result, note and wake, the epilogue included. A
trace passes only if every line matches. A line the replayer cannot drive is a failure, never a
pass, and a trace that exercises a question the design has not settled does not count until it
is settled. Usage is compared under the model's placement of page tables. Traces include the
timer-driven rows of the completion table (a taken call timing out, a budget deadline firing),
run on the real timer. The case passes when 100,000 model traces replay with identical results.

Replay is what turns the model from a reference into evidence about the kernel.

**Open:** how the replayer gets `init`'s boot handles and devices without an interface that exists only for testing; how `tick`, `irq`, `fault` and `record` lines are produced on the real machine; the boot sizes (the model's fixed limits against the kernel's, which follow RAM) and the rv32 cost table (one saved-context page); `map_device`'s result (the kernel returns the address and the length, the model only the address); `map_anon` once the kernel's placement window is full, where the model still succeeds; a receive record made unwritable while its thread waits, which the model refuses to put in a trace ([follow-up](../todo/receive-output-late-invalid.md)); completion races between harts, which a sequential trace cannot express; scheduling and IRQ masking, which results do not show.

## Residual risks

- **The model checks its own abstraction.** Properties that hold and mutations that are caught
  show that the model agrees with itself and that its checks bite on it. They say nothing about
  the kernel. No trace has been replayed on the real kernel, so every "attacked only in the
  model" on the kernel pages rests on the model and the kernel agreeing, which nothing has
  shown. A model that has drifted from the design keeps passing its own tests; only replay or a
  reader finds the drift.
- **Host tests do not reach the kernel's boundaries.** Timer-driven cancellation, the kernel's
  locking and completion races between harts are outside the model: a step is atomic and time
  is a counter. Passing model runs establish none of them.
- **The kernel's `map_anon` window can run out where the model's does not.** The kernel places
  `map_anon` only within a 256 MiB window from its default base. A process can fill that window
  with `map_fixed`, and its own later `map_anon` calls fail with `OutOfMemory` where the model,
  which searches all of user space, succeeds. This harms only the calling process, and it is
  kept as a known divergence ([memory](memory.md)). A replayed trace that does this will differ.
- **One boot case copies model results by hand.** `tests/programs/src/bin/budget-test.rs` runs a
  short budget sequence whose expected results were read off the model. Nothing re-derives them
  from the model, so a change to the model leaves them stale without a failure.
- **Finite worlds.** The default run is 90,020 sequences on small boots (the testing boot's
  `root` has 1,024 pages and 24 processes), and random record changes target blocked calls only.
  Sizes and mapping geometry bound what the runs explore. The million-sequence run is a separate
  test that the bench does not run.
- **The coverage test for mutations stops at twelve.** `every_rule_has_a_mutation` requires a
  variant only for rules numbered 1 to 12. R13, R14, R21 and I16 have variants, but no test
  requires them to keep one.
- **The steward model is checked only against itself.** Its cryptography is ideal, and the
  non-interference comparison leaves out approving or denying a vault request (approval is
  declassification, by design), ending a vault session, and server crashes. A leak through crash
  blame or a session's end is not checked by it.
- **Rules outside the model** (R15, R16, R17, R19, R23) have no model check at all; their boot
  cases are their only attack.

## Why

- **Independent of the kernel's source.** A model that shared code with the kernel would share
  its bugs. With no dependencies, the model can be read side by side with the design, and every
  failure is reproduced from one seed.
- **Ghost state apart from the kernel model.** The checks read facts recorded at each event from
  the primary objects, never the kernel model's own counters, so a wrong update cannot also
  correct the check that should catch it.
- **Mutations, because a property can hold for the wrong reason.** A check that nothing reaches
  passes every seed. A planted break that no property catches names the gap, and every rule
  keeps at least one planted break so that a weakened check shows up as a surviving variant.
- **Text traces, with an epilogue.** Text is easy to diff, to shrink and to carry into a bundle,
  and a `no_std` parser serves the model and a replayer alike. The epilogue exists because a
  kernel can diverge silently; probing memory, usage and handle tables at the end makes the
  divergence show in a result.
- **The steward on the kernel model, not beside it.** Running the policy on real modelled calls
  means every kernel invariant holds under everything the policy does, and non-interference is
  judged on what the kernel actually returns.
