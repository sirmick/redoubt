# FSD3 case 3: the label check's refusal (architect-11)

Ruling: (a). In a confined boot the refused caller cannot exist: R34 refuses any manifest in which
two entries with differing label sets share an endpoint, so every caller init can hand fsd's
endpoint carries the volume's labels exactly. That refusal is R34's and is already tested
(host:redoubt-init::confined_refuses_two_label_sets_on_one_endpoint, bench:init-refuses-confined-server);
(b) would add nothing. (c) would leave R25 with no boot-level test against a 9P server. So the
refusal is shown where it can happen: an unconfined boot, which is ordinary multi-tenancy.

## The cases

3. **`fsd-confined-labelled`**: a confined manifest with a labelled volume; its `fsd`, carrying
   the volume's labels, writes through `blkd`, and a client with the same labels reads it back.
   (The refusal half moves to case 9.)
9. **`fsd-label-check`** (R25 in a boot): an unconfined manifest; a volume labelled {L} and its
   `fsd`; a client labelled {L} writes a file; an unlabelled client, handed a badge at the same
   endpoint, is refused a read of that file (its walk or open, and a `stat`) and a write. The
   verdict is the refusals and the {L} client reading the file back unchanged.

Both widths, like the rest.

## Page lines

fsd.md, "Volumes, connections and labels": when FSD3's other cases land, the summary becomes

    <details><summary>Status: built · tested (31)</summary>

with these five lines before the 26 host lines (31 = 26 + 5; recount if the host list moved):

    - bench:fsd-boot
    - bench:fsd-confined-labelled
    - bench:fsd-corrupt-volume
    - bench:fsd-label-check
    - bench:fsd-one-volume

(The removed clause named one instance per volume under `init` and the corrupt line under it:
fsd-boot/fsd-one-volume and fsd-corrupt-volume cover them.)

serving.md, R25: the summary becomes

    <details><summary>Status: built · tested (8)</summary>

and `- bench:fsd-label-check` goes in beside `- bench:net-attacks`. The removed clause said only
`ipd`'s refusal was tested in a boot; fsd-label-check attacks the 9P skeleton's check in one.

No prose changes: fsd.md's "Labels are per volume" bullet and R25's text already state the rule.
