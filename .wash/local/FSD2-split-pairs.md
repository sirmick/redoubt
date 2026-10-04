# FSD2: how many pairs one littlefs commit can add (for SPLIT_PAIRS)

littlefs `compact` (libs/littlefs/src/fs.rs:541): limit = min(bs-40, bs/2) = 2048 at bs 4096.
Loop: find split by halving the entry COUNT (split += (end-split)/2) until the suffix
entries[split..] totals <= limit; move that suffix to a new pair; repeat on the prefix; stop
when the whole prefix fits (split == 0). A pair compacts only when an append does not fit its
log (bs-8 = 4088 bytes, superseded tags and CRCs included), so live entries at compaction are at
most ~4088 bytes plus the commit's own attrs (set_attr: up to 1022+4).

Simulation of that exact loop (FSD2-split-sim.py, random entry-size lists):
- totals <= 4140, entries 30-100 bytes: up to 2 new pairs
- totals <= 5120, entries 30-100 bytes: up to 3 new pairs
- totals <= 4140, mixed (some ~1 KiB attrs): up to 4 new pairs, e.g.
  [1024,1068,1000,220,66,226,58,56,87,78,62,72] (4017 bytes)
- totals <= 5120, mixed: up to 5 new pairs, e.g.
  [47,73,54,73,1080,1036,65,38,46,48,1050,1056,74,219,45,69] (5073 bytes)

Removes: a delete commit with a full log compacts; what is left can still be over limit, so
it splits. But `compact` falls back when `new_pair` fails with NoSpace: everything stays in
one pair (fs.rs:568). So no commit (remove or otherwise) NEEDS a new pair while its entries
fit one block; splits take free blocks opportunistically, and fsd's recount charges them.
