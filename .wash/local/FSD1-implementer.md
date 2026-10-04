# FSD1: `fsd`, one volume served over 9P, on the host

Tier A (it parses a medium and files other principals wrote). Size M. It needs nothing and can
start now: everything it builds runs on the host. Run every cargo and bench command as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

The file server step is three packages, and this is the first:
- **FSD1** (this one): the server on one volume, its labels, typed operations, remove, mounting.
- **FSD2**: quotas per attach root (R48).
- **FSD3**: `fsd` under `init`. That covers the range badge minted from `volumes`, `blkd`'s
  range labels, the image's disk, restart, and the bench cases, R47's among them.

Do none of FSD2's or FSD3's work here.

## Context rules (read these first; context ran out four times on INIT2)

- **Don't read whole files.** Run `grep -n`, then Read a range.
  - `libs/rt/src/server/ninep.rs` is long; you need the `FileServer` trait (from
    `pub trait FileServer`, about 120 lines), `serve_with` and `mint_rooted`.
  - `libs/littlefs/src/*.rs` total 3,000 lines; you need only the `pub fn`s of `Filesystem` and
    its file and dir handles (`grep -n "pub fn"`), and `lib.rs`'s module comment.
- **Don't open `.wash/qa/*.md`, other packages' reports or other briefs.** If you must open a QA
  file, read it only up to its checkpoint comment: `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Use `cargo testbench --list | awk '{print $1}'`. Read logs only through
  `grep` or `tail`.
- **Read a file right before you Write it,** and prefer Edit.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/FSD1-report.md`.
- **If you hand off, keep the handoff short** and end it with "what consumed my context". Your
  successor reads that handoff and this brief, not the reading list again.

## Reading list (only these)

- `docs/servers/fsd.md`: "Volumes, connections and labels", "Typed operations", "littlefs" (its
  prose, not its test list), and "Failure and restart".
- `docs/servers/serving.md`: "The 9P server skeleton" and "Replies and rollback", only their
  prose.
- `servers/bootfsd/src/server.rs` (283 lines) and `src/bin/bootfsd.rs` (74 lines): the model of
  a 9P server on the skeleton, its host tests (`server_tests.rs`), and its `Cargo.toml`.
- `libs/wire/tables/fsd.md` (the four typed operations and their errors) and
  `libs/wire/tables/blkd.md` (`info`, `read`, `write` and `flush` on a range).
- `libs/client/src/file.rs` and `fsd.rs`: only their `pub fn`s, which say what a client already
  assumes.

## What is settled (cite these; reopen none)

- **One `fsd` per volume:** R47, and fsd.md's first bullet. It is not a choice to make again.
- **Labels per volume:** each node's labels are the volume's, so the skeleton's R25 check runs on
  every request, and there are no per-file labels or permission bits.
- **The four typed operations, and remove:** exactly as fsd.md states them, with the wire table
  as written.
- **Files survive a reboot in M1.** fsd.md's purpose says so; FSD3 builds the image's disk and the
  two-boot case. All FSD1 needs is the mounting rule below.

## The rules this brief settles (with their page lines)

1. **Arguments and handles.**
   - `fsd` gets one named handle, `volume`: its range badge at `blkd`.
   - Its arguments are `labels=ID[,ID...]` (the volume's label set, absent when it is empty) and
     `buckets=N`.
   - It parses them strictly, as every server does, and exits with `BAD_ARGS` before serving on
     anything it does not understand.
2. **Blocks.** littlefs blocks are 4096 bytes, eight of `blkd`'s 512-byte sectors, with no
   argument. The block count is the range's sector count (from `blkd`'s `info`) divided by 8. A
   range of fewer than 4 blocks (littlefs's minimum) is refused as `NO_VOLUME`, before serving.
3. **Mounting.**
   - At start `fsd` mounts its range.
   - A range whose first two blocks (the superblock pair) read as all zero has never been
     written, so `fsd` formats it.
   - Any other range that does not mount is served as corrupt: every attach gets the 9P error
     `corrupt`, and `fsd` stays up and prints one line saying so. A hostile or damaged medium
     never becomes a restart loop, which under `init` would reboot the machine.
   - `fsd` never formats a range that holds anything.
4. **`corrupt`, a new typed error.** fsd's error table gains row 8, `corrupt`. A typed operation,
   or a 9P request, that meets littlefs's `Corrupt` or an I/O error from `blkd` answers
   `corrupt` (the `Rerror` text for 9P). After an I/O error `fsd` answers `corrupt` to every
   request until it is mounted again, as fsd.md's failure section already says. A client can tell
   a broken volume (`corrupt`) from a refusal (`refused`, `not_found` and the rest).
5. **Remove.**
   - Every other fid on a removed file gets `removed` on its next read, write or stat; only a
     clunk succeeds. littlefs keeps a removed file readable through open handles, and `fsd`
     does not use that.
   - A node holds no resource (the skeleton's rule), so tell removed from live by a generation:
     for example, the node carries the file's id and a generation that the remove bumps.
6. **Typed operations' fids.** They name the caller's fids, which live in the skeleton.
   - Add one public resolver to `libs/rt/src/server/ninep.rs`: a caller's fid to its node, the
     same lookup a 9P request uses, so a stranger's fid is `not_found` and never another
     connection's.
   - Give it a host test. Change nothing else in `libs/rt`.
7. **What `fsd` does not do in FSD1:**
   - it meters no bytes: `minted` accepts any quota and FSD2 builds it;
   - no `init` and no bench boot.

### Page lines (exact; each one in the commit that makes it true)

**fsd.md**, "Volumes, connections and labels". After the "One instance per volume" bullet, add:
> - **Arguments.** `fsd` gets one named handle, `volume`, its range at `blkd`, and the arguments
>   `labels=ID[,ID...]`, the volume's label set, and `buckets=N`. littlefs blocks are 4096 bytes,
>   eight of `blkd`'s sectors, so the volume's block count is its range's sectors divided by 8.
> - **Mounting.** At start `fsd` mounts its range. A range whose first two blocks are all zero
>   has never been written, and `fsd` formats it. Any other range that does not mount is served
>   as corrupt: every attach is refused with `corrupt`, and `fsd` stays up, so a damaged or
>   hostile medium never becomes a restart loop. `fsd` never formats a range that holds anything.

**fsd.md**, "Typed operations". After the "Attributes." paragraph, add:
> **Corruption.** An operation that meets a corrupt volume, or an I/O error from `blkd`, answers
> `corrupt`, for 9P and the typed operations alike, so a client can tell a broken volume from a
> refusal.

**fsd.md, statuses.**
- "Volumes, connections and labels" becomes "built · partly tested: one instance per volume
  under `init` is FSD3's · tested: …".
- "Typed operations" becomes built and tested.
- Quotas, Authority, R47, R48 and Failure and restart stay planned.

**libs/wire/tables/fsd.md**: row `| 8 | \`corrupt\` |`, and whatever the wire generator and its
checks need with it.

## The tests

All of them run on the host, in a new case `fsd-host-tests` (kind `host-tests`, as
`bootfsd-host-tests` is). Name each one in its section's status list.

- **The skeleton's tests,** against a block device in memory standing in for `blkd`'s range:
  attach, walk, open, create, read, write, read_dir, stat and remove, through 9P as a client
  sends it.
- **Labels:**
  - with `labels=7`, a reader holding {7} reads and one holding {} gets nothing;
  - only a caller holding exactly {7} writes;
  - a directory listing never shows an entry the caller may not read.
- **Remove:** after a remove, the file's other fids get `removed` on read, write and stat, and a
  clunk succeeds (fsd.md's attack test).
- **The typed operations:**
  - rename within the volume, and a directory into itself refused;
  - `copy_file`'s byte count;
  - `set_attr` refusing types 0 to 15 and values over 1022 bytes (`too_large`);
  - a stranger's fid refused, and fids on two connections refused (the client library's
    rule).
- **Mounting:**
  - a zeroed range is formatted;
  - a range of noise is served as corrupt and `fsd` keeps answering;
  - a mounted volume whose block device starts failing answers `corrupt` from then on.
- **The client library, unchanged, against the real `fsd`:** `File` and `fsd::rename`,
  `copy_file`, `get_attr` and `set_attr`, the way API1's tests ran them against in-test servers.
- **Admission:** a client's connections and fids are bounded by `buckets` (R26), and
  `disconnect` frees its fids.

Mutation-check the label check, the removed generation and the mount rule. Each must fail a test
when broken; say how you checked.

## Owned paths

- `servers/fsd/**` (new), and the root `Cargo.toml`'s workspace members.
- `libs/rt/src/server/ninep.rs`: the one resolver in rule 6 and its test. Before you commit it,
  tell the orchestrator in one line, because RT1 owns `libs/rt` (its sites are elsewhere).
- `libs/wire/tables/fsd.md`, and the wire generator's output for the `corrupt` row.
- `libs/littlefs`: only a fix for a bug `fsd` finds, with a test that fails before it. Report
  each one.
- `tests/fsd-host-tests.toml`, and `fsd`'s entries in `tests/size-budget.toml` and
  `tests/unsafe-budget.toml`. `fsd` has no `unsafe`: `#![forbid(unsafe_code)]`.
- The fsd.md lines above.

**Not yours:** `servers/init` and `servers/blkd` (INIT3, INIT4 and FSD3), `image/`,
`tools/testbench`, `libs/client/src` (call it, don't change it), and the rest of `libs/rt`.

## Gates

- `fsd-host-tests`, `littlefs-host-tests` and `client-host-tests`.
- The whole bench on both widths, alone (one whole bench at a time).
- `cargo fmt --check`.
- The size budget: give `fsd`'s lines.
- The unsafe ratchet.
- doccheck.

Report each command with its exit code. The report lists each rule above with the code and the
test that shows it, and each page line as written.

## Checkpoint

When attach, walk, read and write pass through 9P against the in-memory range, send one progress
line with the branch. Then go on.
