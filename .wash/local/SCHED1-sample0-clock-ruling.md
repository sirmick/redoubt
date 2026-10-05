# SCHED1 sample 0: clock containment diagnosis

Assignment `2de23c01deea0d26dbe19ece62c06328`; QA `IPC3-wake-latency`.
Read-only diagnosis of the preserved seed-3 rv64 run, not a replay or a new verdict.
No source, tests, old ruling, book page or preserved evidence was changed; no test or
QEMU was run. This file is the deliverable.

## Finding

The archived observations do **not decide whether the physical RTC deadline was
before the declared release**. The conservative bound rejects correctly: containment
is unproved. There is also a separate, conclusive obstruction: the unchanged latency
and audit interval starts 27 us before release. An improved physical-deadline proof
alone cannot qualify this sample under all the existing gates.

Do not report an early physical deadline, a scheduler defect, a latency result, or a
passing construction from this evidence. Do not remove the interval gate to make a
new physical-deadline witness sufficient.

## Exact evidence and identity

Read `.wash/local/evidence/SCHED1/go-boundary-host-correction.md` and the authoritative
`.wash/local/SCHED1-relative-wait-ruling.md`, including its inclusive clock proof.
The latter and `SCHED1-layout-ruling.md` continue to govern qualification.

The preserved machine source is HEAD
`7b23f52835f7b6141f0ed7405e0ef5eb2a3d2097` plus tracked patch SHA256
`63b87f63ecbbf6625b0bd68dfaa9002990158a3b9c612217622acc0c5265f987`.
The host-only correction is separately recorded as patch
`fe3ae2110cb0af522e39d25e3dc4bf8821a1a44445cbd643d0b3b5a7dd91d52e`;
it did not produce another machine run.

I checked these hashes directly against the archived patch and log, and checked that
the live guest source still equals the preserved run's recorded identity:

- `tests/programs/src/sched.rs`:
  `180dba7c5f5c23b1964d4f24077f5278bb8da253556f3a08f4f1044c93ba149c`.
- `tests/programs/src/bin/sched-cluster.rs`:
  `7d7d8e9746a366a9bef044fc25c1dc321ee011cf18a7282be8a167310ba450d4`.
- `.wash/local/evidence/SCHED1/run-1-1791164421199359362/sched-cluster-rv64-smp1.log`:
  `969cfa247ffa83945c1849b0d45baeb9c7aa16a08f46ccb0c59d1b81a3b65a01`.

Log line 70 declares `[920644, 16920644]` us. Lines 128–129 report:

| Field | Sample 0 |
| --- | ---: |
| Intent / nominal slot offset | zero / 0 us |
| Preparation slip | -170528 us |
| RTC arming read A | 1791164427679528808 ns |
| RTC deadline D | 1791164427850157808 ns |
| RTC service S | 1791164427886425000 ns |
| Programmed delay | 170629 us |
| Existing kernel sample end E | 956884 us |
| Gross latency | 36267 us |
| Kernel post-service read P | 957012 us |

Source `sched.rs:425–444` reads the arming counter, computes the one delay, reads RTC
A, sets `D = A + delay * 1000`, waits for the interrupt, obtains E with `time_now`,
reads RTC S, then obtains P with another `time_now`. Hence D-A is exactly the reported
170629000 ns. No stored kernel timestamp brackets A directly.

The trace has driver budget 84's validated go W at sequence 3789, K at 3798, measured
wait D at 3804, its first measured W at 5170, service K at 5415, and next D at 5421.
At the first wake, the preceding audit records are U/V 5167/5168; after W the trace
has U/V 5171/5172 and timer I 5173. The spinner cluster follows at 5199–5229. These
are ordering observations, not a timestamp of the RTC deadline. In particular,
`SCHED1-layout-ruling.md` forbids attributing a driver IRQ W to the nearby timer I.
The later service K likewise is not a clock reading. I did not requalify all 200
joins or continue into coverage/latency analysis.

## What the clocks establish

`S-D = 36267192 ns = 36267.192 us`; the required ceiling is 36268 us. Thus
`L = E - ceil((S-D)/1000) = 920616`, 28 us below release. The existing floor-based
latency interval starts at `E-gross = 920617`, 27 us below release.

The inclusive checks `S >= D`, `E <= P`, and `P <= fixed_end` hold for this sample.
That does not establish its lower boundary.

Let K_s denote real-valued kernel-clock time at the RTC service read and K_d that
clock at the RTC deadline. Retain the existing monotonic same-rate virtual-clock
assumption, not equal epochs. Then `K_d = K_s - 36267.192`. Because `time_now` floors
to integer microseconds (`docs/kernel/timer.md`, Clock; kernel
`arch/riscv/timer_sbi.rs:47–56`), the observed bracket gives:

`956884 <= K_s < 957013`

and therefore:

`920616.808 <= K_d < 920745.808`.

This interval crosses release 920644. A service observation at least 27.192 us after
the numerical E would put the physical deadline at or after release; a smaller gap
would put it before release. The recorded P-E=128 us brackets the entire E-to-P
sequence; it is **not** a measurement of E-to-S. We cannot allocate that duration
between syscall return, RTC access, the next syscall, or possible scheduling delay
from these timestamps. The trace does not independently timestamp S. No assertion
that exactly 128 us, or exactly 28 us, was spent before S is justified.

There is a second mapping limitation in the construction. `Bench::go`,
`sched.rs:2092–2103`, reads `(now_t, now_u) = (ticks(), time_now())`. It sends child
start/end from the earlier raw-counter read but reports start/end from the later
kernel-clock read. Raw `rdtime` and kernel time have different origins: the kernel
subtracts BOOT_TICKS. Even at the correctly reported 10 ticks/us, those two reads
are not simultaneous. `prep_us` is a rounded difference against the child's raw
slot; it is not a kernel timestamp of A. Zero-intent arithmetic ensures a nominal
100-us offset relative to that child's counter-based release, but without bounding
the setup read gap it does not prove a 100-us offset from **printed** release.
This corrects the tentative nominal-offset inference made during diagnosis. The
recorded data neither prove that this gap caused the failure nor exonerate it.

## Bounded ruling and smallest sound observation change

The existing conservative proof is sound, but insufficient here. The smallest
additional physical-deadline witness is a successful kernel `time_now()` B **before
the RTC arming read A**, retained in ordered metadata. Under the same-rate assumption,
checked `B + delay_us` is a lower bound on K_d when `D-A = delay_us * 1000` is checked.
Preemption between B and A makes this bound conservative; it cannot make it falsely
late. An explicit proof may use the maximum of this bound and the original L, while
retaining the inclusive service bracket and its fixed-end check. This proves the
same physical containment property without an epoch assumption or changed deadline.
It must not retroactively invent B from `prep_us`, P, or a pass value.

That observation is a recommendation for a separately coordinated source revision,
not a claim that adding it cures this run or guarantees the next one. Preserve
E, S, gross, audit subtraction and the `E-gross >= release` interval check. Even if
an added witness proves K_d is inside, this archived sample still fails the latter
check. The parser should distinguish the two reasons in diagnostics. It must not
clamp the interval, subtract 27/28 us, move E to P, move the release, change the
100-us phase, exempt sample 0, or retain a qualifying subset.

With the currently required E and gross fixed, no observation-only addition can
change `956884-36267 < 920644`. Accordingly **no complete passing repair of this
archived sample exists within the stated constraints**. A prospective construction
must establish both the physical containment and the unchanged audit-window
containment. The preserved evidence does not establish such a construction; do not
claim that the extra B read guarantees it or tune an arming margin around this run.
Any proposal to change the measurement endpoint/order or interval definition needs
a separate scoped design ruling with its audit and negative-control argument.

If B is implemented in a later authorized revision, version metadata and parser
together; reject missing/failed/mismatched B, checked-arithmetic overflow, invalid
deadline arithmetic and inconsistent timestamp order. Exercise a case where the new
physical proof passes but the unchanged latency interval is outside, and require
rejection. An unsupported same-rate clock is still unsupported. Neither adding
instrumentation nor documenting this recommendation authorizes a machine run.

All other requirements remain: fixed 80-ms slots, 200 attempts and exact blocked
D/W/service joins per stand-in, the two exclusive X/Y fences, unchanged phases and
intent, all category/debt/rank minima, all 16 spinner wakes/spread, all latency and
audit gates, 64 MiB and zero drops, identical candidate/control construction, and
qualifying old-10-ms controls that fail latency on both widths. No scheduler policy,
tolerance, retry or sampling exception is introduced. QA remains blocking; the
orchestrator controls any further assignment, review and machine release.
