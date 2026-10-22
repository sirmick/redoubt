# The test bench

## Verdicts

### Rule F (verdicts)

Status: built · tested: bench:smoke

Verdict lines come only from the bench.

The docs checker's rule C4 finds process references; C12 checks the table of contents.

The boot cases, and 2 of the 6 run under `icount`: 1 so, one with a disk at `icount =
"shift=3"`. What keeps the other 4 on the host's clock, a case counted under each it has: a disk
or a userland disk (1 cases); a `forward`, a peer or a dial (1); `[[input]]` (1; and one of the
1 with none of these. A boot case without `icount` (four of them) keeps the host's clock. A
loopback case (1 of the 1) passes as a verdict.
