# FSD4 report

Branch wp-fsd4, worktree /home/mcloonan/redoubt/.worktrees/fsd4, from main 53bcd9704.

## Step 1: the window and the host tests (committed 9b16e4146)

Commit 9b16e4146, "fsd: a directory is listed a window at a time". It changes:
- `servers/fsd/src/server.rs`:
  - `WINDOW = 64`, with the bound in its comment.
  - `Window` and `Listed`, with `holds`, `keep` and `entry`.
  - `Fsd.changes` (the change generation) and `Fsd.window`.
  - `fill` replaces `entry_name`: it finds the directory, makes one `read_dir` pass that skips
    invalid names, skips to `index`, and keeps up to 64 entries. Each kept entry has its name,
    its id (missing or malformed: corrupt), its kind, its size and, for a file, its qid version.
  - `dir_entry` serves from the window when the directory (path and id) and the generation
    match and the index is in the window, or the directory ends inside it. It still checks that
    the volume is not poisoned (`with(|_| Ok(()))`). Otherwise it refills. No `node_at` or
    `stat_of` per entry.
  - The generation moves at the top of `recounted`, which every change runs through: create,
    write, truncate, remove, rename, copy and set_attr. `next_id`'s write to the root's counter
    attribute is not listed.
  - A test-only `passes` counter.
  - The skeleton and littlefs are unchanged.
- Tests:
  - `server_tests.rs`: helpers `entries`, `listing`, `entry`, `many` and `names_in`; the tests
    `listing_a_directory_reads_it_once_per_window` and
    `one_reply_costs_a_bounded_number_of_passes`.
  - `typed_tests.rs`: `a_change_between_reads_refills_the_window` (create, remove, rename and a
    write between two reads; the rest of the listing equals a fresh listing from the same
    index; a directory removed mid-listing gives `removed`).
- `docs/servers/fsd.md`: the bullet, the residual clause and three status lines (31 to 34).
- `tests/size-budget.toml`: servers/fsd ceiling 1343 -> 1401. The window costs 58 code lines;
  the commit carries `Size budget: servers/fsd: the listing window and its change generation`.
  **Flag:** this file is not in the brief's owned paths. The size rule asks for exactly this
  line, but say if you want it handled otherwise.

### E and P
- The shortest stat is 50 bytes: 41 fixed, a 1-byte name (2+1), and empty uid, gid and muid
  (2 each).
- At most `MSIZE - IOHDRSZ` = 65536 - 24 = 65512 bytes of data fit in one reply.
- So E = floor(65512 / 50) = 1310, and P = ceil(1310 / 64) + 1 = 22.
- The skeleton asks for one entry past what fits, so the tight bound is ceil(1311 / 64) = 21,
  within 22.

### Measured (host)
- 600 entries, 4096-byte reads: 10 passes (bound 11). 660 block reads for the whole listing; one
  pass is 60 block reads, so this is (passes + 1) x one pass. The old path's per-entry lookups
  would be on the order of 600 passes.
- 1400 entries with 5-byte names, one read at MSIZE: 1213 entries and 19 passes (bound 22).

### Gates at the tip, exit codes
- `in-dev cargo testbench fsd-host-tests`: 0 (53 passed)
- `formatting`: 0
- `docs`: 0
- `size-budget`: 0 after the commit
- `unsafe-budget`: 0. fsd has 0 unsafe before and after (no unsafe added).
- `fsd-build` (rv64 and rv32): 0
- `bootfsd-build`: 0
- `cargo +nightly fmt -p redoubt-fsd` applied.

### Page lines (exact)
- Bullet: "**A directory is listed a window at a time.** `fsd` serves a directory read from a
  window of up to 64 entries with their stats, filled by one pass over the directory and dropped
  by any change to the volume. Listing n entries costs about n / 64 passes and no lookup per
  entry, and one request costs at most 22 passes over the directory it reads, 22 fixed by the
  largest message."
- Residual: "**Large directories and files scale poorly** in littlefs's format; a listing is
  linear in the directory."

## Step 2: the bench's generated files (0d25fdd69), per the orchestrator's answer B

- `tools/testbench/src/disk.rs`: a littlefs partition may say
  `generated = { files = N, read = "fNNN" }`.
  - The pack makes f0.. in the volume's root, zero-padded to the last name's digits.
  - All are empty except `read`, which holds its own name and a newline.
  - They sit beside the stage's tree; a name in both is refused.
  - A littlefs partition needs a stage or generated files, and `read` must be among them.
  - Noise is now decided by `fs = "noise"`, not by a missing stage.
- New host test `testbench::a_recipe_can_generate_a_directory_of_files`.
- `docs/testbench.md` gains a paragraph and a status line (10 to 11).
- Gates: `in-dev cargo testbench host-tests` exit 0 (it also ran blkd, client, fsd and init
  host tests), formatting 0, docs 0.

## Step 3: the case (882aacb6c, not run on the machine yet)

- `tests/fsd-large-directory.toml`, both widths.
  - Its manifest is `tests/data/fsd/large-directory.json`: keyd, consoled, blkd, fsd, a lister
    and a reader.
  - Its recipe is `tests/data/fsd/large-directory-disk.toml`: 4 MiB, one littlefs partition,
    `generated = { files = 600, read = "f000" }`.
- `in-dev cargo testbench --pack-disk` on the recipe: exit 0, and all 600 names are in the image.
- `fsd-client` gains two modes:
  - `list`: timed from its first read to its last. After its first read it sends to the reader
    and waits for the reader's answer. It checks 600 entries, all distinct.
  - `reader`: waits for the lister, reads /f000 (it must hold "f000\n"), says so, and sends back.
- Verdict lines:
  - `read /f000 during the listing in N us`
  - `listed 600 in N us, in R reads` with N matching `1?\d{1,6}`, so under 2 s
  - `TEST PASSED` (reporter: lister)
- `fsd.md` status gains `bench:fsd-large-directory` (35).
- Not booted: this waits for the host.

## Open
- The window's memory: up to 64 x 255 name bytes plus 64 x 32 bytes, held for the life of fsd.

## Fold of the review notes (tip 1614c6cb5)

The commits are now be03eb44c (fsd), 7045da89f (testbench) and 1614c6cb5 (case). Against
882aacb6c the tree differs only by the folds below.

- Simplifier (1): the noise error now reads "noise is neither staged nor generated" (testbench
  commit).
- Simplifier (2): no helper already did this, but `outsider` repeated the same handle lookup
  inline. The helper is renamed `handed` (`outsider`'s parameter `endpoint` shadowed the old
  name), and `outsider`, `list` and `reader` all use it (case commit).
- Simplifier (3): not changed. `Window::entry` must give back two owned Strings, the Node's path
  and the FileStat's name, and makes exactly those two. The old path made three: entry_name's
  copy, join, and stat_of's copy.
- Simplifier (4): measured with size-budget's own count. Main 53bcd9704 is at 1343; the tip is at
  1401; net +58. The simplifier's ~110 is gross: the removed entry_name and the comments are not
  netted out. The commit line now reads:
  "Size budget: servers/fsd: 1343 -> 1401 code lines, +58 net: the listing window and its change
  generation, less the per-entry path it replaces"
- Red team: a new residual bullet in fsd.md, "A listing across a change may skip or repeat an
  entry". A read goes on by index and a refill is fresh, so a create or remove before that index
  shifts the rest (9P allows it). The window is filled whole, so an unreadable entry up to 63
  places ahead fails the earlier read as `corrupt`, as it would within one reply (links R49).
  It is in the fsd commit.
- Gates at 1614c6cb5, all exit 0:
  - size-budget (1401 of 1401)
  - docs
  - formatting
  - unsafe-budget
  - host-tests (every host-tests case: blkd, client, fsd, the bench's own, init, ipd, littlefs,
    model, net, netd, r4, rt, sshd, steward, stride and wire)
  - fsd-build and bootfsd-build on rv64 and rv32

## Machine case, on the host alone (tip 8f45767ec on main 128bd45d9)

- The first boot failed on both widths: "a PASSED line not the reporter's". The reader printed
  its own TEST PASSED, and the bench takes a PASSED line only from the reporter.
- Fix (folded into the case commit): a new `Ends::Said` makes the reader say its line and park,
  with no verdict of its own. The case's verdict is the lister's.
- `in-dev cargo testbench fsd-large-directory`: exit 0.
  - rv64: listed 600 in 505,852 us (0.51 s), in 8 reads; the reader's read was answered
    mid-listing in 55,959 us.
  - rv32: listed 600 in 513,924 us (0.51 s), in 8 reads; the reader in 58,097 us.
  - Case wall time: 1.8 s on rv64, 2.0 s on rv32.
  - For comparison, BEAM2 found 587 entries unlisted after 400 s.
- `in-dev cargo testbench fsd-`: exit 0, 23 PASS.
  - Booted on both widths: fsd-boot, fsd-confined-labelled, fsd-corrupt-volume,
    fsd-label-check, fsd-large-directory, fsd-one-volume, fsd-quota, fsd-reboot, fsd-restart.
  - Also passed: bootfsd-build and fsd-build on both widths, and fsd-host-tests.
- After the fold, formatting and docs exit 0. The fold changes only how the reader ends; this is
  the tree that booted.
- The listing time under the whole run is pending your bench.
