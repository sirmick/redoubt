# B11 ruling (Architect): the memory scan waits for every server's record

The implementer's three facts (`.wash/local/B11-report.md`) are accepted: the stub touches the
stack before the record is written, `init-boot` has no per-server console lines to witness a
start, and `expect` is ordered. The first ruling's option (a), the console-line witness, is
withdrawn; the `init-boot` stop-line move is moot. (B) is rejected: it names one server in one
case, and the next late server races the same way.

## The rule: (D), stated as a readiness condition

A memory case's dump is taken when two things hold: the case's stop line has appeared, and
every declared server has written its heap record. The runtime writes the record before
`main`, so the record is the one true witness that a server started. The bench learns the
second only from a dump, so a dump missing a record lets the guest run on (QMP `cont`) and
dumps again, until every record is present or the case's deadline passes; `forbid` lines stay
in force meanwhile. A record still missing at the deadline fails the case: a declared server
that never started is a true failure, and one `init-boot`'s console verdict alone did not
catch (it has no beamlet line); this closes that gap rather than excusing it. Duplicates,
out-of-range units and an unreadable dump fail at once, as today. The cost is one more dump
per retry on the slow path only; the run's output says how many dumps and how long it waited
("memory: dumped twice, waited N s for beamlet").

## Is the peak still the peak?

The record's peak is monotone, so a later dump reads a number at least as large, and the
verdict (a cap at least twice the peak) is only made stricter by waiting. What changes is the
moment: from "the stop line" to "the later of the stop line and the last server's start",
which is what the table's six-run rule always intended, servers that had started. The page
says so; the rule is the same on both widths.

## The page sentence

testbench.md, "The memory budget", first paragraph, in place of "After its console verdict,
the bench stops QEMU over QMP and dumps the guest's physical RAM beside the case's log, as
`<case>-<arch>-smp<N>.ram`.":

"After its console verdict the bench stops QEMU over QMP and dumps the guest's physical RAM
beside the case's log, as `<case>-<arch>-smp<N>.ram`. The runtime writes each server's heap
record before `main`, so the record is the witness that a server has started: a dump missing a
declared server's record lets the guest run on and is taken again, until every record is
present or the case's deadline passes, when the case fails. A server's peak is therefore the
most pages its heap had held when the last declared server had started, or later, never
earlier than the stop line."

The first ruling's "not started" sentence is dropped entirely.
