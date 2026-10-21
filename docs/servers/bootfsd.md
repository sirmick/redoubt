# bootfsd

`bootfsd` serves `/boot`: the programs and module archives the boot manifest marks public, as one
flat, read-only 9P directory every session can read. It never sees the boot bundle. Its launcher
hands it the public entries' bytes, one at a time, and then seals it; after that nothing can
change, and nothing else in the bundle, the manifest least of all, is ever in its address space.

## Purpose

Sessions need the system's programs and modules (a VM's `.beam` files, for one) without a file
server holding the whole signed bundle, which carries `keyd`'s seeds and every principal's keys in
its manifest. `bootfsd` gives them exactly the published entries: the answer to "what may a
session read from the bundle?" is the list it was built from, not a filter it applies.

## Interface

### Serving `/boot`

<details><summary>Status: built · tested (11)</summary>

- bench:r4-host-tests
- bench:bootfsd-build
- host:redoubt-bootfsd::a_session_reads_the_public_entries_and_sees_nothing_else
- host:redoubt-bootfsd::a_walk_to_an_unpublished_name_is_the_same_as_to_one_that_never_existed
- host:redoubt-bootfsd::entries_read_back_byte_for_byte_at_any_offset
- host:redoubt-bootfsd::every_way_of_writing_is_refused
- host:redoubt-bootfsd::the_directory_lists_exactly_the_public_list_in_order
- host:redoubt-bootfsd::the_conformance_vectors_run_against_bootfsd
- host:redoubt-bootfsd::serving_connection_rolls_back_discard_missing_capability_and_error
- host:redoubt-bootfsd::the_image_entry_holds_its_buckets
- host:testbench::every_manifest_entry_holds_its_buckets_at_their_caps

</details>

`bootfsd` is a 9P server over the [9P server skeleton](serving.md#the-9p-server-skeleton), and its
endpoint also serves `ninep_common` ([wire](wire.md#ninep_common)).

- **Flat.** Every connection attaches at `/boot` itself, whatever the attach name; the root holds
  one file per published entry, in the order of the `public` list, and nothing else.
- **Exact names.** A walk finds a name from the `public` list, byte for byte, and nothing else. Any
  other name is "does not exist", the same answer as a name the bundle never held, so `/boot` says
  nothing about the rest of the bundle.
- **Read-only.** `Twrite`, `Tcreate`, `Tremove` and `Twstat` are refused, and so is opening for
  writing or with `OTRUNC`: the read-only rule does not rest on the label check, which an
  unlabelled caller passes. There is no code that changes an entry once sealed.
- **Unlabelled.** Every entry carries no labels, so every caller, labelled or not, may read it.
- **Every byte is already there.** A read never waits; an offset past the end reads nothing. Entries
  never change, so every qid version is 0.
- **Admission** ([R26 (admission fairness)](serving.md#r26-admission-fairness)): at most 47 open
  fids (the 23 connections' roots, each a fixed cost outside the share, and a pool of one working
  fid per connection and one over, an entry walked, opened, read and clunked:
  [serving](serving.md#admit)), 23 minted connections, 2 multiplexed connections' completion calls, 64 of their requests
  and 2 pages they brought ([serving](serving.md#multiplexed-connections)) per (account, label
  set), and per badge for account 0, across at most `buckets=N` of those at once, sized to fit
  the budget its manifest entry gives, the entry's pages less its heap cap and stack, by the entry
  rule ([walfsd](walfsd.md#serving)). The connections are what a manifest lets one account hold
  at `/boot` at once, whoever minted them ([serving](serving.md#admit)): one per session the
  steward starts, the console session, the cap of live contexts per label set and a spare, and
  one per lease, the leases what the principal's unlabelled share holds in `agent` budgets
  ([the steward](steward.md#leases)), held by the bench to at least the largest such sum over
  the tree's manifests, (1 + 3 + 1) + 18 under `steward-latency`'s
  (`the_servers_admit_what_a_manifest_lets_one_account_hold`). A bucket at its caps costs
  173,568 bytes; the image needs
  five (`init`, the steward, and alice's `{}` and `{alice-secrets}` and bob's `{}` domains) and
  asks six, a case's extra client the sixth, 1,041,408 bytes or 255 pages, which with the heap cap's
  2,510 (below) and the stack's 128 round to 3,072 of the image's 4,096; the entry leaves 1,458
  pages of budget, thirty-four such buckets, so each more principal's domain costs one more; a
  block with no `buckets=N` or no budget, or a count the budget cannot hold, and `bootfsd` does not
  start ([init](init.md#the-boot-manifest)).
- **Heap** ([Heaps](init.md#the-boot-manifest)): the cap is twice the heap's peak at boot, where
  the published entries live, and once beside it the most that admission lets clients make the
  skeleton hold on the heap. Those records are the heap's, though their units are costed in the
  budget ([serving](serving.md#admit)), so a peak measured at boot leaves them out, and a cap of
  twice that peak alone holds them only while clients happen to hold few. At a bucket's caps the
  skeleton holds, in rv64's sizes, which rv32's do not exceed:

  | Held | Records | Bytes each | Bytes |
  | --- | ---: | ---: | ---: |
  | requests | 64 | 216 | 13,824 |
  | completion calls, a session each | 2 | 784 | 1,568 |
  | minted connections | 23 | 384 | 8,832 |
  | fid holders, one a fid at most | 47 | 192 | 9,024 |
  | the bucket and its shares, one a connection | 1 + 24 | 192, 200 | 4,992 |

  38,240 bytes in tables that grow by doubling, so at most twice that, 76,480; each fid's own
  blocks, its slot in its holder's list (256, the most per fid, a list of one in the 256-byte
  class) and its steps (128), 18,048; a transfer's record (64) for each of its 2 pages, 128; and
  the old block of a table while it moves, at most the requests', 13,824: 108,480 bytes a bucket.
  Once: the read buffer, a 64 KiB reply's (`MSIZE`) and its move, 32 pages; a page of rounding for
  each of the six tables and the moving block, 7; and a part page in each of the allocator's 8
  small classes ([the native runtime](../userland/native.md#redoubt-rt-the-native-runtime)): 47
  pages. For `buckets=N` that is ⌈108,480 × N / 4,096⌉ + 47 pages: the image's six 159 + 47 = 206,
  and its cap 2 × 1,152 + 206 = 2,510, where 1,152 is the boot peak its base is set from. Its
  measured peak is 1,141 pages on rv32 and 935 on rv64, the same alone and beside eight other
  cases, so the cap rests on the derived bound, not on a measured load. Every manifest's `bootfsd`
  entry adds the same to twice its boot peak: four buckets 153, five 180, ten 312. One allocation is
  outside it: a send's T-messages are listed before admission takes their pages, and a transfer of
  `MAX_LEND_PAGES` of 7-byte messages lists 9,362 of 40 bytes, 240 pages with its move, from any
  session, though a send of more than 256 ends its session. The second copy of the boot peak holds
  it, so nothing fails, but a case that sent it would fail the bench's check of twice the peak; none
  does.

### Filling it

<details><summary>Status: built · partly tested: the refusal of `add` and `seal` from badge 0 is read from the code, not attacked · tested (7)</summary>

- host:redoubt-bootfsd::a_bad_public_list_stops_the_server
- host:redoubt-bootfsd::the_public_list_is_checked_before_anything_is_served
- host:redoubt-bootfsd::add_only_appends_to_a_listed_name_in_order
- host:redoubt-bootfsd::setup_is_refused_after_seal_and_from_every_minted_connection
- host:redoubt-bootfsd::nothing_is_visible_before_seal
- host:redoubt-bootfsd::a_client_cannot_publish_into_boot
- host:redoubt-bootfsd::the_published_bytes_are_bounded

</details>

- **The list.** `bootfsd`'s arguments are the `public` list, one entry per argument, in the
  manifest's order, each `LENGTH:NAME`: the entry's length in bytes as its launcher read it from
  the bundle, a canonical number (no leading zero, `0` itself allowed), then a colon, then the name, split at the
  first colon so a name may hold one. Each name must be one 9P path component (not empty, `.` or
  `..`, no `/` or NUL), named once, at most `MAX_ENTRIES` (64), and the lengths together at most
  `MAX_BYTES` (8 MiB). Each entry's buffer is reserved for its length once, here, so the server's
  heap holds the entries and never a growth step: before, an entry grew by doubling from its
  first chunk, and its peak (the old and the new buffer together) depended on the pusher's chunk
  size and on where the image fell against a power of two. A list breaking any of that, or one
  the heap cannot hold, stops the server before it serves anything: a `/boot` that is not what
  the manifest named is worse than none.
- **`add(name, offset, data)`** appends `data` to a listed name. `offset` must be exactly what has
  been added to that name so far, so a chunk cannot be lost, repeated or reordered, and the
  chunk must end within the entry's declared length.
- **`seal`** ends setup. Before it the directory is empty and every walk is "does not exist", so no
  client sees a half-written entry; after it `add` and `seal` are refused for good. A seal while
  an entry is short of its declared length is refused and `/boot` stays empty: the launcher,
  which cannot finish the boot without it, says so, rather than a file that is not what the
  manifest named being served.
- **Only the founding handle fills it.** Both messages are refused (`refused`) from badge 0 and
  from any connection `new_connection` minted (a badge at or above `FIRST_MINTED_BADGE`), so only a
  holder of a root badge, handed out when the server was set up, can publish.

The table: [libs/wire/tables/bootfs.md](../../libs/wire/tables/bootfs.md).

{{#include ../../libs/wire/tables/bootfs.md:tables}}

### Started by `init`

Status: built · partly tested: no session's launcher is started, so no fresh connection is rooted at `/boot` for one · tested: bench:init-servers, bench:init-boot, bench:init-refuses-public-manifest

`init` starts `bootfsd` from the bundle's pages with the manifest's `public` list as its
arguments, each entry with its length from the bundle, reads the bundle itself, pushes each
public entry's bytes with `add`, and sends `seal`
([init](init.md#starting-the-servers)). `init` refuses a manifest whose `public` list names an
entry the bundle does not hold, or the manifest itself. Sessions get fresh connections to
`bootfsd`, rooted at `/boot`, from their launcher.

## Authority

Status: built · tested: host:redoubt-bootfsd::a_client_cannot_publish_into_boot, host:redoubt-bootfsd::every_way_of_writing_is_refused

`bootfsd` holds its endpoint and the connections it minted, and the bytes it was handed. It holds
no handle to the bundle, no device and no connection to any other server, and it makes no calls.
Its clients get read access to the published entries and nothing more; its founding handle's
holder gets `add` and `seal` until the seal.

## Security properties

### R46 (only the public list)

<details><summary>Status: built · tested (6)</summary>

- host:redoubt-bootfsd::a_session_reads_the_public_entries_and_sees_nothing_else
- host:redoubt-bootfsd::a_walk_to_an_unpublished_name_is_the_same_as_to_one_that_never_existed
- host:redoubt-bootfsd::setup_is_refused_after_seal_and_from_every_minted_connection
- host:redoubt-bootfsd::nothing_is_visible_before_seal
- host:redoubt-bootfsd::every_way_of_writing_is_refused
- host:redoubt-bootfsd::a_client_cannot_publish_into_boot

</details>

`/boot` shows exactly the entries of the `public` list, with exactly the bytes the founding
handle's holder added before the seal, and nothing else: nothing before the seal, no change after
it, no publishing from any minted connection, and the same answer for a name outside the list as
for one that never existed. The rest of the bundle, the manifest included, never enters
`bootfsd`, so no bug in it can serve them.

## Failure and restart

Status: built · partly tested: that a restarted `bootfsd` serves nothing until sealed again follows from its start (an empty, unsealed table) and is not attacked across a restart · tested: host:redoubt-bootfsd::a_bad_public_list_stops_the_server, host:redoubt-bootfsd::serving_connection_rolls_back_discard_missing_capability_and_error

- **A bad `public` list** stops `bootfsd` with an exit code before it serves.
- **A connection whose reply is lost** is rolled back ([replies and rollback](serving.md#replies-and-rollback)).
- **`bootfsd` restarts:** it starts empty and unsealed, and serves nothing until its launcher fills
  and seals it again.

## Residual risks

- **Whatever is public is public to everyone.** Every session, labelled or not, reads every
  published entry; a secret put on the `public` list is a secret published.
- **A restart empties `/boot`** until its launcher fills it again; who does that after boot is
  [init](init.md#restarts-and-reboots)'s.

## Why

- **Never see the bundle.** A server that holds the bundle and filters it serves the manifest the
  day its filter has a bug; a server that was only ever handed the public entries cannot.
- **`add` at an exact offset.** A lost, repeated or reordered chunk would publish wrong bytes that
  every session trusts; an offset that must match makes each of them a refusal.
- **Empty until sealed.** A session that reached `/boot` while it filled would see a half-written
  module; an empty directory until the seal means it sees all of an entry or none.
