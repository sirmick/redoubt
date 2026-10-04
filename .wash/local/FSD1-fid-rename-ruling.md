# FSD1-fid-rename: ruling (architect-8)

## Ruling

1. **(A): a fid is {path, id}.** Every op resolves the path and checks the id; absent or
   different is `removed`. Fids do not follow renames. (B)'s table grows with every node a client
   walks to (littlefs has no stable number to key it on and drops nodes unseen), which is memory a
   client drives and state to audit; (A) needs nothing beyond the fid. No page promises Plan 9's
   rename-following.

2. **No entry is ever without its id, so no read path writes.** The order of a create:
   - first commit: the root's counter moves past the new id;
   - second commit: the entry is created **with its id attribute in its creating commit**.
   A power cut between them burns an id, which is harmless (ids are never reused, not dense).
   This needs one littlefs addition: `mkdir` and a creating `open` take initial user attributes
   that go in the creating commit (the same tag list, `TYPE_USERATTR | typ` at the entry's id).
   The id written by `copy_file`'s destination is the same path. A rename keeps the id (its
   attributes move with the entry).

3. **Mount checks ids without writing.** After a mount, one walk: every file and directory has an
   id, no two the same, and the counter is above the highest. A volume that fails is served as
   corrupt, under the existing Mounting rule (every attach refused, `fsd` stays up). So `fsd` serves
   only volumes it wrote; a C-written or hostile image is corrupt as a whole.

   Gone with this: `give_id`, transient ids and `TRANSIENT`, `find`'s transient arm, and the
   revised "id on first touch" rule. Red's BLOCK (a read writes the counter: R25 and fsd.md's
   "a write needs them equal") is closed because nothing but a create moves the counter, and a
   create needs equal labels. Red's P2s close too: the counter is checked at mount, and no
   `NoSpace` can strand an entry. Red's other P1 (blkd's `read_only` flag) is separate; a
   read-only range now mounts and serves with no write at all.

## Tests the fix round owes

- littlefs: a crash at every write of a create-with-attributes leaves no entry without its
  attributes (the crash harness, files and directories).
- littlefs or fsd: a power cut in a rename between two pairs leaves a volume whose mount check
  passes (the pending move's source is not counted as a second entry with the same id).
- fsd: a walk, a stat and a directory read by a reader with a superset of the volume's labels
  write nothing (the device's write count unchanged).
- fsd: an image with an id-less entry, a duplicate id, or a counter at or below the highest id is
  served as corrupt; one written by fsd mounts.
- fsd: a fid on a renamed file, and below a renamed directory, gets `removed`.

Rename `an_entry_without_an_id_gets_one_when_first_reached` to what it now tests.

## Page lines (fsd.md, in FSD1's commit)

**Volumes, connections and labels, "Mounting" bullet**, append after "`fsd` never formats a range
that holds anything.":

> A range that mounts is then checked, without writing: every file and directory carries its id,
> no two the same, and the id counter is above the highest; a volume that fails is served as
> corrupt too. So `fsd` serves only volumes it wrote.

**New bullet, after the 9P bullet:**

> - **A fid is a path and an id.** Every file and directory has an id, `fsd`'s own attribute and
>   its qid path, from a counter on the root that never gives one twice. The counter moves in one
>   commit and the entry is created with its id in the next, so a power cut skips an id and never
>   leaves an entry without one, and no read writes the volume. Every operation on a fid finds its
>   path again and checks the id: an entry gone, or with another id, is `removed`. So a remove or a
>   rename over a file ends every other fid on it, and a fid on a renamed file, or on anything
>   below a renamed directory (a connection's root included), is `removed` as well: fids do not
>   follow renames.

**Attributes paragraph:** "(mtime, qid version, and later use)" becomes "(the id, mtime, qid
version, the root's id counter, and later use)".

**littlefs, `Filesystem` bullet:** "user attributes on files and directories" becomes "user
attributes on files and directories, which a create can write in its own commit".

**Why**, new bullet after "Labels per volume, not per file.":

> - **Fids do not follow renames.** Following them needs a table of every file a client has walked
>   to, keyed on a number littlefs does not keep; a path and an id need no memory beyond the fid.

## Later (not FSD1)

`docs/userland/files.md`'s list of Unix habits that do not carry over gains "a file open across a
rename of it or its directory" when the client's `File` is built against fsd.
