# The memory scan's duplicate rule: ruling (Architect)

Found by EROFS1's rv32 `userland-boot`: `beamlet: stack paint unit 6763 found twice`, two
pages each holding only that one painted word at the unit's in-page offset, neither beamlet's
stack page. Options offered: (a) a residual; (b) count a unit only in a page holding at least N
painted words; (c) a contiguity rule from the stack's frames.

## What the paint proves

testbench.md "The memory budget": the launcher paints each stack with a tag and a unit index;
the scan reads tags "at their encoded offsets within physical pages", which "ignores paint
words copied into ordinary stack slots"; "the lowest missing unit marks the stack's deepest
touched point". A painted word proves that stack word was never written. A copy of it
elsewhere proves the same thing about the same word, so it is not a second unit. The duplicate
rule exists for forgery and corruption. A 14 s boot merely stopped overwriting stale copies,
so this is the scanner's known blind spot showing: a bench defect, not a residual (a), and not
EROFS1's fault.

## The rule: count a stack page's units from its maximal physical page

The unit index fixes each unit's stack page k = unit / 512 and its in-page offset. For each k
the scan counts units only from the one physical page holding the most of page k's units at
their offsets; a unit found in another page is a copy and is ignored. This never undercounts
the deepest touched point: a copied buffer is a subset of the words of the real page, which
still holds every untouched one of them, so the real page is maximal. Two pages holding equally
many of page k's units (a whole stack page copied intact, or a forgery) are still refused as a
duplicate.

Why not (b): a threshold N fails on a copied painted buffer of N words. Why not (c) by frames:
it needs the guest's page tables parsed by the bench, more machinery than the rule above for
the same proof.

## The page sentence

testbench.md, after "...refusing a missing server, a duplicate unit or an out-of-range index at
an encoded offset.": "A stack page's units are counted from the one physical page that holds
the most of them, so a stack word copied into a buffer or a message is not a duplicate; two
pages holding equally many of a stack page's units are refused."

## Who

A B package (bench only): `tools/testbench/src/memory.rs` and its scanner tests (a copied
single word ignored; a copied buffer ignored; an intact page copy refused; the lowest missing
unit unchanged by copies), size S, launchable now and ahead of EROFS1 in its train. EROFS1 does
not take `memory.rs` into its paths; its rv32 `userland-boot` console verdict stands, the scan
clause reruns once the B package merges; the kept dump is the evidence.
