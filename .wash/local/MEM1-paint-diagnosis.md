# MEM1 rv32 paint identification diagnosis

Assignment `595383cfa562da670b3fb65cbaa5bade`, QA `MEM1-runtime-stack`.
Read-only analysis of preserved evidence; no guest execution, source changes, tests,
or final stack sizing. This report is the only new file.

## Evidence identity

- Archive: `.wash/local/evidence/MEM1/MEM1-cal-userland-boot-rv32.tar.gz`.
  Verified SHA256 `4e2edd2002f5c91b53ef4db54f0443476f217027d487b6b54ed617c5ff8ac069`.
- Archived RAM member: `target/testbench/run-1-1791162320073185698/userland-boot-rv32-smp1.ram`.
  Exactly 1,073,741,824 bytes; computed SHA256
  `e6915f3f36c9155029283ba75a62548f3e790c5a7d818c5f44f20f478181406b`.
- Archived source record names HEAD `6535a9acbd6eea8fcdc7142d291da0d95ed7a656`.
  Verified calibration-patch SHA256
  `07090083475360102f81ed2e3911ca64a69d07f984d13d2e4151441ae1eb229e`.
  Current HEAD matches; only the recorded four calibration paths are dirty.
  Scanner, launch and paint-definition sources are unchanged from that HEAD.
- Archived console reaches the shell, expected module refusals, and result `55`.
  That guest verdict does not make the failed memory scan a pass.

## Offending bytes and provenance

`stub/src/lib.rs:104` defines each eight-byte little-endian unit as
`0x5354414b << 32 | tag << 16 | index`. `libs/client/src/launch.rs:205`
paints indices from the bottom of the stack. Beamlet is manifest tag 10, with
8192 units in the provisional 16 pages. QMP dumps physical RAM from `0x80000000`
(`tools/testbench/src/qemu.rs:614`).

A complete aligned-word search finds exactly one recognized-tag out-of-range
candidate: dump offset `0x946c98`, physical address `0x80946c98`, bytes
`54 53 0a 00 4b 41 54 53`, word `0x5354414b000a5354`.
Its apparent index is `0x5354 = 21332`. Its physical page slot is 403, whereas
`21332 % 512 = 340`: it cannot be an original paint unit at this address.
No duplicate in-range, page-congruent unit was found in the complete dump.

Nearby aligned words are:

| Physical address | Word |
| --- | --- |
| `0x80946c78` | `0x5354414b000a138f` |
| `0x80946c80` | `0x60cdf65000000002` |
| `0x80946c88` | `0x5354414b0000059d` |
| `0x80946c90` | `0x414b000a137760cd` |
| `0x80946c98` | `0x5354414b000a5354` |
| `0x80946ca0` | `0x5354414b000a1394` |

At the unaligned address `0x80946c92`, the eight bytes are exactly
`77 13 0a 00 4b 41 54 53`, a valid tag-10 paint word for index `0x1377`.
Its final two bytes overlap the rejected word's index. Thus a shifted paint-like
sequence plus surviving destination paint explains the false header. The original
slot at `0x80946c98` would hold index `0x1393 = 5011`, not 21332.
The presumed original location of index `0x1377`, `0x80946bb8`, is already
overwritten; the dump cannot prove the historical copy operation or its writer.

Independent address evidence: a coherent Sv32 chain has candidate root
`0x80512000`, entry 511 = `0x20253401`, pointing to leaf table `0x8094d000`.
Its entries 1008 through 1023 map virtual `0x7fff0000..0x80000000` to physical
`0x8093d000..0x8094d000`, each with flags `0xd7` (V/R/W/U/A/D).
The rejected address corresponds to virtual `0x7fff9c98`, inside that stack.
The same leaf table's startup mapping points to `0x8094e000`, whose strings
include `budget_pages=24576` and `Elixir.Redoubt.Shell`, matching beamlet.
Surviving tag-10 indices independently agree with these stack frames.
This is strong structural attribution, not an independently recorded active SATP
or a complete audit of kernel allocation ownership.

## Finding and smallest correction

`tools/testbench/src/memory.rs:79` checks index bounds before its existing
physical-page-offset qualification at line 87. It therefore raises an ambiguity
error on a word that its own untouched-paint qualification would reject.
An ordinary stack write may leave some paint bytes intact; a public magic/tag
prefix does not prove that the remaining index is meaningful.

The smallest proposed correction is to apply the existing page-offset congruence
check before indexing or checking the candidate against its declared stack length.
This follows the existing launcher invariant: virtual and physical page offsets
are identical, even when physical frames are not contiguous or ordered.
The dump base is page aligned. No new tolerance, paint encoding or guest behavior
is needed. A mismatched word credits no unit; the overwritten original slot stays
missing and therefore contributes conservatively to measured use.

This is not permission to ignore all out-of-range words. A recognized-tag,
page-congruent out-of-range candidate must still fail. Preserve duplicate qualified
unit failure, missing-paint failure, manifest bounds, restart/fault guards and all
other ambiguous candidate failures. Keep this invariant explicit in review.

Before a new guest run, the implementer should obtain coordinated source/review
authorization and demonstrate through the prescribed testbench path: the exact
partial-overwrite pattern above credits no paint; an out-of-range index placed at
its matching page offset still fails; duplicates and missing paint still fail;
existing valid scans retain their behavior. Review the actual corrected scanner
against the preserved dump before requesting a machine slot. These are proposed
checks, not checks performed in this diagnosis.

## Limits and acceptance

The evidence establishes a false candidate under the scanner's existing spatial
criterion. It is consistent with normal stack residue and unaligned copying; it
does not prove that the guest operation was legitimate, identify a function, or
exclude a runtime corruption/undefined-behavior defect. It supplies no evidence
of a larger-than-declared stack or a needed page-count increase. Determining the
historical writer would require separately scoped execution evidence; it is not
necessary to establish the page-offset contradiction.

No peaks or final beamlet declaration are derived here. The failed scan remains
failed pending corrected validation. All six complete workloads, strict twice-peak
sizing, final-declaration remeasurement, bounds/docs and review remain required.
The valid rv64 fsd:system peak of 12072 bytes remains calibration evidence and a
margin failure, not acceptance. `MEM1-runtime-stack` remains blocking.
