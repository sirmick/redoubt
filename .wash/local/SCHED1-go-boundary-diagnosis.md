# SCHED1: readiness acknowledgment mistaken for go

Assignment `beae021f277c201ff478ee38fe0be020`, QA `IPC3-wake-latency`.
Read-only trace/source diagnosis; no source edits, tests, QEMU or rerun. This report
is the only new file. Sequence numbers below are trace sequence numbers, not times.

## Preserved identity

Run: `.wash/local/evidence/SCHED1/run-1-1791164421199359362/`.
Console SHA256 verified:
`969cfa247ffa83945c1849b0d45baeb9c7aa16a08f46ccb0c59d1b81a3b65a01`.
Source HEAD from the preserved record: `7b23f52835f7b6141f0ed7405e0ef5eb2a3d2097`.
Preserved tracked patch SHA256 verified:
`63b87f63ecbbf6625b0bd68dfaa9002990158a3b9c612217622acc0c5265f987`.
Current `tests/programs/src/sched.rs` and `tools/testbench/src/sched_oracle.rs`
match the recorded hashes `180dba7c...149c` and `080989a1...df7` respectively.
The fixture inspected is the preserved `candidate-seed3-rv64-source/sched-cluster.rs`.
Trace sequences are contiguous 0 through 148481; terminal record reports zero drops.

## Source protocol

The parent starts 19 children serially and consumes each zero readiness report before
starting the next (`sched-cluster.rs`). `Bench::start` passes all handles directly
through `process_start`: report in slot 1, go in slot 2, extras from slot 3
(`sched.rs:2068`, `spawn.rs:182`). The driver's RTC/IRQ/marker occupy slots 3/4/5;
the timer's marker occupies slot 3. There is no later marker-delivery message or
marker receive wait.

Each `cluster_child` first calls `report(0)`, which is a FOREVER **blocking send**
on slot 1 (`sched.rs:216,376`). Only after it returns does the child call `window()`,
a FOREVER receive on slot 2 (`sched.rs:222,377`). After all readiness reports,
`Bench::go` sends one window message to every child in start order (`sched.rs:2092`).
Thus readiness may itself produce a D/W pair before the actual go receive.

## Independent order replay

The last 19 first-W budgets map, in isolated start order, to server 33, spinners
36 through 81 in steps of 3, driver 84 and timer 87. Each is picked before the next
first W. These IDs are observations, not proposed constants for the oracle.

| Event | Driver 84 | Timer 87 |
| --- | ---: | ---: |
| Spawn W | 3093 | 3205 |
| First child K | 3102 | 3214 |
| Readiness-send D | 3108 | 3220 |
| Launcher K before acknowledgment receive | 3109 (budget 1) | 3221 (budget 1) |
| Readiness-completion W | 3115 | 3227 |
| Child resumes K | 3124 | 3236 |
| Window-receive D | 3130 | 3242 |
| Actual go W | 3789 | 3822 |
| First measured D/W | 3804 / 5170 | 3835 / 5197 |
| Last measured D/W | 104326 / 104332 | 104310 / 104319 |
| Last service K before fence | 104341 | 104391 |
| Marker X/Y | 104350 / 104367 (ID 31) | 104400 / 104417 (ID 32) |

The oracle calls the **second** W go (`sched_oracle.rs:878`); here those are the
readiness completions 3115/3227. Consequently, the extra counted pair for budget 84
is D3130/W3789: the actual window receive and go delivery. The analogous pair for
87 is D3242/W3822. The readiness-send pair itself is before the selected boundary.
This distinction matters: the fix is not to discard an unexplained measured wait.

Actual go is independently anchored by the protocol and the collective trace:
after the final spawn W3205, server 33 first wakes at 3249. All 19 actual go wakes
then appear in parent send order:

`3249, 3285, 3314, 3343, 3382, 3411, 3440, 3469, 3508, 3537, 3566, 3605, 3634, 3663, 3692, 3731, 3760, 3789, 3822`.

Every observed readiness completion precedes 3249; all but the final child's also
precede the next child's spawn. Each actual go W follows its child's window D and
has launcher budget 1 as the latest K. The source has no other child-waking action
in this parent phase. The final readiness W3227 precedes server-go W3249, closing
the otherwise missing last-child setup boundary.

Between each **actual** go W and its own X there are exactly 400 D/W records,
alternating D then W in 200 pairs. Every W has a service K before the next D or X.
There are 200 metadata records per stand-in. No report wake is used: X/Y still
precede the reporting code. Marker IDs are attributed by the running caller and
exclusive source call sites, not handle values or destruction order.

The same ordinal error affects spinners. Their third W is actual go, not common
timeout release. The real release comprises all 16 W at sequences 5199..5229
(step 2, reverse spinner order), entry 1868, all before K5234. Their pass values
are identical `7018391798`. Merely changing stand-ins to third-W go would leave
the spinner-release oracle wrong.

## Smallest proposed correction and proof obligations

Keep the guest construction and its reliable blocking readiness sends. Correct
the host's setup/go state machine for all roles; do not substitute a new global
fixed wake ordinal. Anchor the go phase using the server's first W after the final
isolated child spawn, justified by serial readiness consumption and the server's
exclusive window receive before its spinning/reporting phase. Validate that the
anchor belongs to the launcher-driven go phase, not a later report completion.

For every child, validate the source-consistent prefix: spawn/pick, either a fully
completed readiness D/W or an immediate readiness send, then the window D and its
launcher-delivered go W. Any readiness W must precede the go-phase anchor (and the
next child spawn for nonfinal children); require the post-readiness pick before
window D where readiness blocked. Require the 19 go wakes in start order, the
appropriate running launcher provenance, and no unexplained D/W in that prefix.
If this decomposition is missing or ambiguous, fail; do not fit it to a desired
200-sample count. A go receive that never blocks cannot supply its required D/W.

Use those validated per-child go boundaries for both stand-in joins and spinner
release identification. A spinner's next validated timeout D/W after actual go
is its release; retain the sixteen queued wakes before a pick and spread gate.
Retain exactly 200 measured D/W/service cycles before each exclusive X, all ordered
sample/window checks, and the existing marker-attribution proof. Count reporting
only after X. Reject extra measured waits, missing/immediate measured waits,
199 waits plus report W, extra/late readiness pairs, out-of-order go and ambiguous
start or end boundaries. Review both immediate and blocking readiness paths so
scheduling variation cannot silently change the interpretation.

The preserved run supplies a concrete protocol witness for this correction, not
permission to accept 201 cycles, truncate events, or exempt an arbitrary pair.
Apply the identical corrected oracle to candidate and old-slice control. No phase,
deadline, spinner, fixed-window, debt/rank rule, coverage count, latency target,
audit subtraction or fence changes are proposed. The new parser/state machine
requires implementation and independent review before another machine release.

## Limits

This diagnosis replays setup and D/W/K/fence order. It is not a replacement for the
full oracle's rank, clock containment, audit, debt, coverage or latency gates, and
does not claim they will pass. W/K are not physical timestamps. Guest PASS is not
host acceptance; the preserved result remains the original failed run. No further
run is needed to establish this boundary bug. `IPC3-wake-latency` remains blocking.
