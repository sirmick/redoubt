# architect-8 notes

Owed from architect-7 (architect-7-notes.md) stand: FSD2/FSD3 briefs; the M1 page edit at the
init step's close; the held M2 line at K21's merge.

## FSD1 (ruling: FSD1-fid-rename-ruling.md)

Watch at FSD1's merge, beyond architect-7's list:
- the fsd.md lines in the ruling file (Mounting, "A fid is a path and an id", Attributes,
  littlefs `Filesystem`, Why);
- no `give_id`, no transient ids; ids written in the creating commit; the mount id check;
- the five tests in the ruling file.

Checked FSD1's page lines at 970f7ddfd: they stand (nit: fsd.md:137 over 100 columns).

## RT2 (node added: the serve helper)

At merge: serve used by bootfsd, keyd, blkd, fsd, echo-server; consoled, netd, ipd keep their
loops with close_delivery; RECEIVE_FAILED in redoubt_rt::exit; serving.md line from the node.

## K16-churn-ceiling (ruling: K16-churn-ceiling-ruling.md)

Shell variant's ceiling dropped (R12 is a floor; carving over-charges). At K16's or K21's merge
(whichever first): bounds (500-TOL, 1000), scheduling.md Inheritance sentence and Responsiveness
numbers, "650 and 643" sentence gone; K16's evidence run (entry-wait lift fails the oracle).

## SMP1-design: answered (many harts by design; SMP1, SMP3, SMP2; targets 1+2 gated, 4 recorded)

Done: TENETS Harts + m2 step 4 on main (082e00ccf); SMP1 brief updated (status, rule 6 interim
predicate, rule 7 shootdown to a set, page lines); nodes SMP1/SMP3/SMP2 set.
FSD2 and FSD3 briefs written (FSD2: subtree quotas, nothing stored; pair-alias fix; FSD3: range
labels as blkd args, mkimage via fsd's code, no volume check at restart).
Owed: SMP3's and SMP2's briefs (SMP2 restates R12 across harts with several runners).
The held K21 line (architect-7-notes) is DROPPED: K21-free-cost ruled a free-frame bitmap, so no
kernel data lives in a free frame. SMP1's brief now says the shootdown protects the next owner's
data.

## K21-free-cost (K21-free-cost-ruling.md)

At K21's merge: bitmap with fixed-depth summaries (depth a constant from PHYSMAP_SIZE), no frame
written by free/take, kernel_frame takes the highest, bitmap memory counted at boot, audit
replaces check_free_list, alloc-first-fit negative kept; memory.md lines per the ruling; R10
p50/p99 vs main (~18.7/~22.4) reported; main's m2 step 4 kept (not K21's).

Later: `docs/userland/files.md`'s Unix habits gain "a file open across a rename of it or its
directory" when the client's `File` is built against fsd.
