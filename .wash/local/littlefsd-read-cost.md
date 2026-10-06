# littlefsd's read cost on writable volumes: a design note (Architect, for the owner)

Why now: PACK1 measured about 0.9 s per open-and-read of a 1.9 MB file on littlefs and about
6.5 block reads per data block; BOOT1 measured that 97 % of the boot's block reads were root
directory lookups. The system volume is moving to EROFS (EROFS1), but `/home` and `/vault`
stay littlefs (the owner's decision: littlefs for every writable volume), and BEAM3 puts users'
`File.*` on them. Without a change, a session's file reads will feel slow in proportion to the
file's size and the directory's.

## What littlefsd keeps per fid today

A fid is a `Node { path, id, dir }` (`servers/littlefsd/src/server.rs:138`): a path from the
volume's root, the entry's id and whether it is a directory. No open handle, no location, no
cache. Every request re-derives everything:
- `read` (server.rs:915) calls `on_file(node, read, ...)`: `Filesystem::open(path)` (a lookup
  from the root: one metadata pair fetch per directory level, and on a large directory a walk
  of its pair chain; a fetch is three block reads), then `seek(offset)` and `read`, then close.
- `seek` and `read` in `libs/littlefs` follow the file's CTZ skip list from its head to the
  block at `offset`, several pointer-word reads per block boundary, each a whole block read
  through `verityd` on a verified volume, and `libs/littlefs` keeps "one block-sized buffer per
  metadata fetch, one block per file handle that is writing" (littlefsd.md "littlefs",
  Memory): no block cache, no read cache across calls.
- `write` adds a `stat` of the path and the quota arithmetic (`room`, `recounted`) before the
  open.
So one 9P read costs one lookup plus one CTZ find plus the data; sequential reads of a file
repeat both per piece. The page states this as a residual ("`littlefsd` looks a file up from
the root for each request on it: three times for a walk, twice for an open or a read",
littlefsd.md "Residual risks") and explains the fid design ("Fids do not follow renames ... a
path and an id need no memory beyond the fid", "Why").

## What the page says about caching

Nothing is cached, by design: the Memory bullet bounds the server at buffers per fetch and per
writing handle; the residual says a read-only server "can resolve a name once and keep the
result", which EROFS1 does for the system volume. Fids deliberately do not follow renames; a
per-fid open handle in `libs/littlefs` would make them (the crate's handles follow renames and
survive removal), so a naive "keep the handle open per fid" changes 9P-visible semantics
(`removed` for a fid whose file was removed) and the page's rationale.

## Two caches that change no semantics

1. **A block cache in the server's `BlockDevice`** (`servers/littlefsd/src/volume.rs:65`,
   below `libs/littlefs`): an LRU of N whole blocks, write-through (a `prog` or `erase`
   updates or drops the block). littlefs reads are deterministic over block contents, so the
   cache is invisible to it; a hostile medium is still read through `verityd` on a verified
   volume (the cache holds checked blocks). What it buys: the CTZ find's pointer reads repeat
   the same chain prefix (the head and the large skips) on every find, and a small directory's
   pair chain fits whole, so most pointer and lookup reads hit. Cost: N pages of heap. N = 16
   (64 KiB) is the proposal; the measurement sets it.
2. **A lookup cache keyed by path**, in the server: path → (the entry's pair, id, kind, CTZ
   head and size), filled by `open`'s lookup and used by the next request on the same path,
   invalidated whole by the volume's change generation that FSD4 already keeps for listing
   windows ("keyed by the directory and a change generation that every volume change moves"),
   so a rename, remove or write anywhere drops every entry: no stale location, no semantic
   change (a removed file's fid is still found `removed` on its next request, because the
   generation moved). Cost: E entries of about 300 bytes (the path and the location), E = 64:
   about 5 pages.
Together, on a small `/home` directory, a sequential read's cost per data block falls from
about 6.5 reads to about one, and an open-and-read of a file costs one lookup, not one per
piece. Numbers to be measured, not promised: the gate is PACK1's own figure (open-and-read of
the 1.9 MB pack on littlefs, before and after) and a `littlefsd` counter line in the
`boot-stats` build (block reads per data block over a sequential read).

## What it costs in `heap_pages`

Today `littlefsd:data`'s heap peak is 9 pages, cap 18; `littlefsd:system`'s 19, cap 38
(testbench.md's table). Both caches add about 21 pages per instance, bounded and fixed at
start (no growth with the volume or the fids), so the caps move to about 60 and 80 by the
six-run rule; the system instance's cost disappears with EROFS1. A per-fid open handle
instead would cost about 1 KB per fid, unbounded by the server (bounded by the fid limit per
connection), and the semantic change above; not proposed.

## Where it belongs

**One S package, FSD5**, in `servers/littlefsd` only (`volume.rs`, the server's lookup, the
counters), `libs/littlefs` untouched, Tier A. Not EROFS1's (a read-only format with its own
resolution), not BEAM3's (the client), not PACK1's (which reads the system volume, soon EROFS;
its one lever, reading the pack backwards, is a client-side mitigation of the same cost and
stays ruled as it is). Needs nothing; best after PACK1's measurement exists so the before
number is on record, and before BEAM3's machine case, so users' first `File.read` is measured
with it. Pages: littlefsd.md's Memory bullet (the two caches and their bound), the residual's
sentence rewritten to what remains (a lookup is still linear in a directory on a miss; the
caches are per instance), testbench.md's two rows. The owner's question is whether a few
seconds per large read on `/home` is acceptable for M1 or FSD5 goes in before BEAM3's case; my
recommendation is before, since it is S and the measurement is already in hand.
