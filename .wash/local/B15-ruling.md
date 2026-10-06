# B15 ruling (Architect): kernel-containment's bystander clause

## (1) It follows the five-cases ruling; it does not wait for RECON1

R12's claim is relative ("A budget's CPU follows its free weight", docs/SECURITY.md:99;
scheduling.md R12), and `.wash/local/SCHED1-five-cases-ruling.md` section 2 settled that share
fixtures judge by ratio of counts with throughput printed beside. The gate's clause judges R12
for the bystander; a gross floor of 783 confounds R12 with the per-switch cost the page already
states as today's cost (scheduling.md:222-223, RECON1's). Being THE gate does not change what
R12 claims; holding the push for a throughput residual the page states would make the gate
judge something R12 does not say. The program's gross count (`SHARE_FLOOR`,
tests/programs/src/bin/kernel-containment.rs:34; the clause at :247-256) is the shape
`sched-large-weight`'s old clause had.

## (2) The new form and the page sentence

The gate's rule is that no verdict comes from a hostile agent (kernel/README.md, "Verdicts"),
and the competing counts are the agents', so a ratio of the program's counts is not available
here. The source is the kernel's trace: the scheduler oracle judges the bystander's charged
runtime over the charged runtime of every budget under `users` in the gate's window, both net
of audits, against its weight's share within R12's 50 per thousand (the share the old floor
encoded, 100/120, now as a ratio of the kernel's own charges). The program keeps printing its
count as useful work, reported beside, with no verdict on it. The window: the program prints
its window's bounds (`time_now`) for the oracle, or the oracle takes the bystander's first to
last pick; the implementer says which and why.

Page, kernel/README.md:177, the row becomes:

"Every pick is in rank order, and the bystander keeps its weight's share of the CPU the kernel
charged under `users`, net of the checked build's audits; its own count is printed beside as
its useful work | R12 | the scheduler oracle over the kernel's trace; the program prints the
count"

SECURITY.md's R12 row: unchanged (the gate is not listed on it). Tier B (the oracle and the
gate program), size S. Not here: the kernel, the slice, RECON1's fix.

## (3) B5's net-of-audits applies

Moving the clause to the oracle gives it by construction (the oracle's share checks are already
net of audits). A gross program count in a checked build credits audits against the bystander,
which is why 758 is not a kernel number.

## Separate finding, not mine to cut

A `[containment] FAIL` line followed by QEMU's exit 0 is reported by the bench as "guest exited
while waiting"; the bench should report the FAIL line it saw.
