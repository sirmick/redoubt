# INIT5: a launch holds at most one batch of the image, and `INIT_PAGES` returns to 1,024

Tier A (`init`'s bound, the client library's launch, a kernel constant). Size S-M. Needs BEAM1
merged: BEAM1 raises `INIT_PAGES` to 2,048 for `beamlet`'s image, and this package brings it
back. Start from main after that merge. Run every cargo and bench command as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

## Context rules (read these first)

- **Don't read whole files.** Run `grep -n`, then Read a range. You need only `place` and its
  callers in `libs/client/src/launch.rs`, and `bound`, `Counts` and their tests in
  `servers/init/src/bound.rs`.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.** If you must open a QA file, read
  it only up to its checkpoint comment: `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Read boot logs under `target/testbench/last/` only through `grep` or
  `tail`.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/INIT5-report.md`.

## Reading list (only these)

- `docs/kernel/budgets.md`, "The tree from the boot manifest": the `INIT_PAGES` bullet.
- `docs/servers/init.md`, "Launching through the loader stub", steps 1-4, and the residual
  "Every child pays for a copy of its image".
- `docs/userland/native.md`, the launch steps (search "copies the program's ELF bytes").
- `docs/kernel/memory.md`, the `process_map` row.

## The problem

`place` (launch.rs) copies the whole image into one fresh `Buffer` in the launcher's own
pages, then moves it into the child with one `process_map`. So the launcher holds the whole
image at once. For `init` that is `root`'s pages: `bound.rs` counts the largest image whole, and
`beamlet`'s image (about 700 pages) set the bound and made `INIT_PAGES` 2,048. For any other
launcher, a session for one, it is a transient cost of the whole image in its own budget.

## The settled design: place in batches

- **`place` copies and moves at most `PLACE_PAGES` (64) pages at a time.** Each batch goes into
  a fresh `Buffer` and moves with `process_map` to `dst + offset`. The child sees one contiguous
  image at `IMAGE_AT`, exactly as now: the startup block, the stub and the child's charges are
  unchanged, since the child already pays for the image pages and their tables.
- **The stack too.** The stack is placed the same way, so a manifest's large stack never sits
  whole in `init`'s pages.
- **A failure mid-way** leaves the batches already moved in the child, which has not started.
  The launch's existing failure path destroys the child's budget, and everything goes with it.
  Check that path, and test it: a launch that fails on its third batch leaves the budget empty
  and the launcher's pages as before.
- **No kernel change, no stub change, and nothing shared.** Each child still pays for its own
  copy (init.md's residual stands: no shared text, by decision). A design that lent the bundle's
  frames to the stub was considered and dropped: it needs a kernel mapping into an unstarted
  process that the kernel does not have, and it would share frames between principals.
- **The bound.** `bound.rs`'s launch term becomes: the stub's pages and tables, plus, for the
  image and for the stack, `min(pages, PLACE_PAGES)` and `tables(min(pages, PLACE_PAGES))`.
  `largest_image_bytes` still feeds it, for an image smaller than a batch. `PLACE_PAGES` is one
  constant, in the client library, which `bound.rs` imports.
- **`INIT_PAGES` returns to 1,024** in `kernel/src/budget.rs`, provided twice the bound that
  `beamlet-boot` prints on either width is within 1,024. If it is not,
  stop and report the bound: don't pick another number.

## The cases

1. **`beamlet-boot`** prints the bound (BEAM1 made it print it). Report it before and after, on both widths.
2. **`init-refuses-bound`** must still refuse at 1,024. If its manifest now fits, grow it
   (more servers or endpoints, not a larger image, since an image no longer counts whole).
3. **A host test in `libs/client`** (the fake kernel): placing an image of 3 × `PLACE_PAGES` + 1
   pages makes four moves, the child's image reads back byte for byte, and the launcher's peak
   pages are one batch. Plus the failure test above.
4. **`stub-launch`** and **`bundle-mapped`** pass unchanged, as does every case that launches
   through `init` (the whole bench).
5. **`beamlet-boot`** (BEAM1's) passes with `INIT_PAGES` at 1,024.

## Page lines (exact; in the commit that makes each true)

- **budgets.md, the `INIT_PAGES` bullet.** BEAM1's added item ("the image of each program it
  starts, copied whole from the bundle through its pages on the way to the child: the largest
  image counts.") becomes:
  >   - one batch of the program it is starting, at most 64 pages of its image and 64 of its
  >     stack at a time, copied through its pages and moved to the child
  >     ([processes](processes.md));
- **budgets.md, the same bullet's sizing sentence.** BEAM1's "With `beamlet`, whose image is the
  largest, the bound is 1,060 pages on rv64 and 1,286 on rv32 (`beamlet-boot` prints it), and
  2,048 leaves 988 and 762 to spare. It is a fixed count, not a share of RAM, because `init`'s
  needs grow with the largest program it starts, not with the machine," becomes:
  >   With `beamlet`, the bound is N64 pages on rv64 and N32 on rv32 (`beamlet-boot` prints it),
  >   and 1,024 at least doubles both. It is a fixed count, not a share of RAM, because `init`'s
  >   needs do not grow with the machine, nor with the size of a program it starts,
  with your numbers, and "at least doubles both" only if twice the larger is within 1,024 (else
  stop and report, as above). Rewrap the paragraph to the page's width.
- **budgets.md**, "`INIT_PAGES` (2,048)" becomes "`INIT_PAGES` (1,024)".
- **native.md, launch step 3:** "It copies the program's ELF bytes into pages and moves them into
  the process as data," becomes:
  > It copies the program's ELF bytes into the process as data, 64 pages at a time, each batch
  > into fresh pages it then moves in, so a launcher never holds more than one batch,
- **init.md, "Launching through the loader stub", step 2:** after "a copy of the program's ELF
  image, read-write;" insert " placed 64 pages at a time;".

## Owned paths

- `libs/client/src/launch.rs` (`place`, `PLACE_PAGES`) and its host tests.
- `servers/init/src/bound.rs` and its tests.
- `kernel/src/budget.rs`, the `INIT_PAGES` line only.
- `tests/init-refuses-bound.toml` and its manifest.
- The page lines above.

**Not yours:** the stub, the kernel beyond that line, `libs/rt` (RT2's `server/`), and
`userland/otp` (BEAM's).

## Gates

- The whole bench on both widths, alone (one whole bench at a time).
- `rt-host-tests`, the client library's host tests, `init`'s host tests.
- `cargo fmt --check`, the size and unsafe budgets, doccheck.

Report each command with its exit code, the bound before and after, the batch count for
`beamlet`'s image, and each page line as written.
