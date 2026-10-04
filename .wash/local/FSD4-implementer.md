# FSD4: a directory listing reads the directory once, not once per entry

Tier A (`servers/fsd`). Size S. Needs nothing: start from main. Run every cargo and bench command
as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

## Context rules (read these first)

- **Don't read whole files.** In `servers/fsd/src/server.rs`: `entry_name`, `dir_entry`, `find`,
  `node_at`, `stat_of`, `qid`, and `with`. In `libs/rt/src/server/ninep.rs`: the trait's
  `dir_entry` and the directory branch of `read` (around 1022-1044). In `libs/littlefs`: only
  `DirEntry` and its accessors.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.**
- **Keep reports under 1900 bytes,** with detail in `.wash/local/FSD4-report.md`.

## Reading list

- `docs/servers/fsd.md`: "Volumes, connections and labels" (the 9P and "A fid is a path and an
  id" bullets) and "Residual risks".
- `docs/servers/serving.md`: the directory-read rule (a read continues only from where the last
  one ended).

## The finding (BEAM2, 2026-10-03)

Listing the userland disk's root, 587 files, had not finished after 400 s, and `fsd` answered no
other client meanwhile. There are two costs per entry:
- the skeleton fills one `read` reply by calling `dir_entry(index)` once per entry, and each call
  walks the whole directory again (`entry_name`, server.rs:426);
- each entry is then found again by path (`node_at`, `stat_of`, `qid`), so every entry pays
  lookups through its parent.

The work is quadratic in the directory's size, and one reply's worth of it sits inside a single
request.

## The settled design

1. **A window.** `fsd` keeps one listing window: a directory's id, a start index, and up to
   `WINDOW` entries (64). Each entry holds everything `dir_entry` returns: the name, the id, the
   kind, the size and the qid version, taken from the `DirEntry`'s metadata and attributes.
   - **Filling it** is one pass over the directory (`read_dir`). The pass skips invalid names, as
     `entry_name` does, skips to `index`, and keeps up to `WINDOW` entries.
   - **Serving from it:** `dir_entry` answers from the window when the directory and the change
     generation match, and the index lies inside it. Otherwise it refills, finding the directory
     once (its id check, so a removed directory is still `removed`). An entry served from the
     window needs no lookup by path.
2. **Invalidation.** `fsd` holds a change generation, moved by every operation that changes the
   volume (create, write, truncate, remove, rename, `set_attr`; one place, beside `writable()` or
   in `recounted`). The window is valid only at the generation it was filled at. `fsd` serves one
   request at a time, so a listing never sees a half-made change.
3. **Memory** is bounded: at most `WINDOW` names of littlefs's longest, reserved with
   `try_reserve` (`NoMemory` as today). It is one window per `fsd`, not one per fid: a second
   listing interleaved with the first only costs refills.
4. **The cost after the change.**
   - Listing n entries costs about n / `WINDOW` passes over the directory, and no lookup per
     entry.
   - One request costs at most ceil(E / `WINDOW`) + 1 passes, where E is the most entries one
     reply can hold at the largest `msize` (the shortest stat). Compute E, state it in a comment
     on `WINDOW`, and report it.
   - The work of one request is linear in the directory it reads, like a walk to a name in it.

The skeleton and littlefs are not changed.

## The cases

1. **Host, fsd** (the fake range counts block reads):
   - `listing_a_directory_reads_it_once_per_window`: list 600 entries. Expect at most
     ceil(600 / `WINDOW`) + 1 passes (count `read_dir` calls, or block reads against one pass's
     reads), and every entry once, in order, with the stats the old path gave.
   - `a_change_between_reads_refills_the_window`: create, remove and rename between two reads of
     one listing. The listing stays consistent with what the skeleton's offset rule allows, and
     a removed directory is `removed`.
   - `one_reply_costs_a_bounded_number_of_passes`: one `read` of the directory at the largest
     `msize`, within the bound of point 4.
2. **Machine, both widths:** `fsd-large-directory`. The case packs a disk recipe with 600 files
   in its root (`--pack-disk`, FSD3's recipe form). A test program lists the root through its
   `fsd` and prints the count and the ticks it took, and a second client reads a file during the
   listing. Verdict lines: `listed 600`, the second client's read answered, and the listing under
   2 s. Report the time alone and under the whole run.

## Page lines (exact text in the report)

- **fsd.md**, "Volumes, connections and labels", a bullet after "A fid is a path and an id":
  "**A directory is listed a window at a time.** `fsd` serves a directory read from a window of up
  to 64 entries with their stats, filled by one pass over the directory and dropped by any
  change to the volume. Listing n entries costs about n / 64 passes and no lookup per entry, and
  one request costs at most P passes over the directory it reads, P fixed by the largest
  message." Write point 4's computed number in place of P. List the new tests in the
  section's status.
- **fsd.md**, "Residual risks", "Large directories and files scale poorly": unchanged, but add
  "a listing is linear in the directory".

## Owned paths

`servers/fsd/src/server.rs` and its tests, the new case and its recipe, the page lines.

**Not yours:** `libs/rt` (the skeleton), `libs/littlefs`.

## Gates

- `fsd`'s host tests (`fsd-host-tests`); the new case and the `fsd-*` cases on both widths; the
  whole bench on both widths, alone.
- `cargo fmt --check`, the size and unsafe budgets, doccheck.

Report each command with its exit code, the pass counts, the case's times, and the page lines.
