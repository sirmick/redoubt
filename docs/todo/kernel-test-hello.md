# A stray file in the kernel's tree

## What

`kernel/test/hello` is a three-byte file holding `foo`. Nothing builds, reads or tests it: no
Cargo manifest, bench case, script or source names it.

## Why it matters

The kernel is the part of Redoubt meant to be read in full. A file in its tree that does nothing
costs every reader the time to find that out, and the no-cruft rule is that nothing is kept
without a use.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on `init`
and the manifest.

## Where

- `kernel/test/hello`.

## Done when

- The file and its directory are gone.
