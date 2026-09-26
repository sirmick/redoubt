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

Status: built · tested: bench:r4-host-tests, bench:bootfsd-build, host:redoubt-bootfsd::a_session_reads_the_public_entries_and_sees_nothing_else, host:redoubt-bootfsd::a_walk_to_an_unpublished_name_is_the_same_as_to_one_that_never_existed, host:redoubt-bootfsd::entries_read_back_byte_for_byte_at_any_offset, host:redoubt-bootfsd::every_way_of_writing_is_refused, host:redoubt-bootfsd::the_directory_lists_exactly_the_public_list_in_order, host:redoubt-bootfsd::the_conformance_vectors_run_against_bootfsd, host:redoubt-bootfsd::serving_connection_rolls_back_discard_missing_capability_and_error, host:redoubt-bootfsd::the_limits_fit_the_budget

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
- **Admission** ([R26 (admission fairness)](serving.md#r26-admission-fairness)): at most 32 open
  fids and 8 minted connections per (account, label set), across at most 16 of those at once
  (`LIMITS`), sized to fit its 256 KiB budget.

### Filling it

Status: built · tested: host:redoubt-bootfsd::a_bad_public_list_stops_the_server, host:redoubt-bootfsd::the_public_list_is_checked_before_anything_is_served, host:redoubt-bootfsd::add_only_appends_to_a_listed_name_in_order, host:redoubt-bootfsd::setup_is_refused_after_seal_and_from_every_minted_connection, host:redoubt-bootfsd::nothing_is_visible_before_seal, host:redoubt-bootfsd::a_client_cannot_publish_into_boot, host:redoubt-bootfsd::the_published_bytes_are_bounded

- **The list.** `bootfsd`'s arguments are the `public` list, one name per argument, in the
  manifest's order. Each must be one 9P path component (not empty, `.` or `..`, no `/` or NUL),
  named once, at most `MAX_ENTRIES` (64). A list breaking any of that stops the server before it
  serves anything: a `/boot` that is not what the manifest named is worse than none.
- **`add(name, offset, data)`** appends `data` to a listed name. `offset` must be exactly what has
  been added to that name so far, so a chunk cannot be lost, repeated or reordered. All entries
  together hold at most `MAX_BYTES` (8 MiB).
- **`seal`** ends setup. Before it the directory is empty and every walk is "does not exist", so no
  client sees a half-written entry; after it `add` and `seal` are refused for good.
- **Only the founding handle fills it.** Both messages are refused (`refused`) from badge 0 and
  from any connection `new_connection` minted (a badge at or above `FIRST_MINTED_BADGE`), so only a
  holder of a root badge, handed out when the server was set up, can publish.

The table: [libs/wire/tables/bootfs.md](../../libs/wire/tables/bootfs.md).

{{#include ../../libs/wire/tables/bootfs.md}}

### Started by `init`

Status: planned · M1 (separation and containment)

`init` starts `bootfsd` from the bundle's pages with the manifest's `public` list as its
arguments, reads the bundle itself, pushes each public entry's bytes with `add`, and sends `seal`
([init](init.md#starting-the-servers)). `init` refuses a manifest whose `public` list names an
entry the bundle does not hold, or the manifest itself. Sessions get fresh connections to
`bootfsd`, rooted at `/boot`, from their launcher.

**Open:** none.

## Authority

Status: built · tested: host:redoubt-bootfsd::a_client_cannot_publish_into_boot, host:redoubt-bootfsd::every_way_of_writing_is_refused

`bootfsd` holds its endpoint and the connections it minted, and the bytes it was handed. It holds
no handle to the bundle, no device and no connection to any other server, and it makes no calls.
Its clients get read access to the published entries and nothing more; its founding handle's
holder gets `add` and `seal` until the seal.

## Security properties

### R46 (only the public list)

Status: built · tested: host:redoubt-bootfsd::a_session_reads_the_public_entries_and_sees_nothing_else, host:redoubt-bootfsd::a_walk_to_an_unpublished_name_is_the_same_as_to_one_that_never_existed, host:redoubt-bootfsd::setup_is_refused_after_seal_and_from_every_minted_connection, host:redoubt-bootfsd::nothing_is_visible_before_seal, host:redoubt-bootfsd::every_way_of_writing_is_refused, host:redoubt-bootfsd::a_client_cannot_publish_into_boot

`/boot` shows exactly the entries of the `public` list, with exactly the bytes the founding
handle's holder added before the seal, and nothing else: nothing before the seal, no change after
it, no publishing from any minted connection, and the same answer for a name outside the list as
for one that never existed. The rest of the bundle, the manifest included, never enters
`bootfsd`, so no bug in it can serve them.

## Failure and restart

Status: built · tested: host:redoubt-bootfsd::a_bad_public_list_stops_the_server, host:redoubt-bootfsd::serving_connection_rolls_back_discard_missing_capability_and_error

- **A bad `public` list** stops `bootfsd` with an exit code before it serves.
- **A connection whose reply is lost** is rolled back ([replies and rollback](serving.md#replies-and-rollback)).
- **`bootfsd` restarts:** it starts empty and unsealed, and serves nothing until its launcher fills
  and seals it again.

## Residual risks

- **Whatever is public is public to everyone.** Every session, labelled or not, reads every
  published entry; a secret put on the `public` list is a secret published.
- **A restart empties `/boot`** until its launcher fills it again; who does that after boot is
  [init](init.md#restarts-and-reboots)'s.
- **`bootfsd` does not boot in the bench.** It is attacked by host tests against the runtime's
  fake kernel (`r4-host-tests`); `bootfsd-build` only builds it for both widths.

## Why

- **Never see the bundle.** A server that holds the bundle and filters it serves the manifest the
  day its filter has a bug; a server that was only ever handed the public entries cannot.
- **`add` at an exact offset.** A lost, repeated or reordered chunk would publish wrong bytes that
  every session trusts; an offset that must match makes each of them a refusal.
- **Empty until sealed.** A session that reached `/boot` while it filled would see a half-written
  module; an empty directory until the seal means it sees all of an entry or none.
