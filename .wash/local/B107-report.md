# B107 report

Base 328bcecff; branch wp-B107, head 9b1903341 (two commits: c5f709aa2 beamlet with its tests,
9b1903341 docs). Worktree .worktrees/B107. Tier B: no platform, allocator or wire contract
changed; the rule is held with fallible reserves against the existing GlobalAlloc
null-on-refusal contract, through B105's `Process::ask` path.

## What each native does now

| Native | Before | Now |
| --- | --- | --- |
| `maps:keys/values/to_list` | `map_entries` Vec copy, then a Vec of results, then `list` growing the heap | `room(n * (2 + entry))` first; list built from the largest key back (`Heap::map_list`), no copy held |
| `maps:from_list`, `maps:from_keys` | `list_arg` copy, pairs Vec, `map_from` inserting one by one (a garbage path of nodes per insert) | pairs held fallibly (`Ctx::held`), `room(6n + 3)`, then `Heap::map_of`: sorted in place (unstable, place breaks ties), deduped keeping the first key and last value as an insert did, tree built balanced: a node a key, no garbage |
| ETS `lookup`, `first_lookup`..., `take`, `lookup_element`, `update_counter`/`update_element`'s copy, `first/next/...`'s key, every `select`/`match`/`match_object` (what `ets:tab2list/1` calls), `select/3` chunks | `copy_into` per object, growing the heap | `room(sum of the objects' cells + 2n)` (`OwnedTerm::cells`, an upper bound on a copy) before any copy; `take` asks while the objects are still in the table; results listed with `room(2n)` first; chunks `room(2n + 8)` |
| `get/0`, `get_keys/0,1`, `erase/0` | entries Vec copy, Vec of tuples, list | `room(5n)` / `room(2n)` first, built from the end; `erase/0` asks before it empties |
| `unicode:characters_to_list/2` | Vec<char> grown by Vec, then a Vec<Term> the size of half the result | the chars Vec grown through `held`; `room(2n + 4)`; list built from the chars directly |
| `binary_to_term/1,2` | decoder pushing onto the heap, Vecs with_capacity, inflate `vec![0; size]` | `Process::steps`: each tuple, list, string, map, binary, bignum, fun asked before it is made (`Steps::allow`: cells via `try_room`, all steps' words counted together against the limits held when the call began; the first 128 words unchecked, as `ask`); element/pair/env copies, binary bytes and a compressed term's inflated buffer `try_reserve_exact`; any refusal is `EtfError::NoRoom` -> `Ctx::refused` (collect and rerun, then kill). Other decode callers (the loader, info) pass no steps: the heap grows as before, but their copies are fallible too |
| `integer_to_list/1,2` | `Heap::string` with a Vec<Term> copy | `room(2 * digits)`; `Heap::string` now builds from the end with no copy (all callers) |
| `list_arg` (every native reading a list: options, `ets:insert`, `--`, `list_to_tuple`, `spawn` args, ...) | `Heap::to_vec`, Vec growth | `list_len`, then `held(n)` (`ask` with 0 words: the allocator only, no limit), then extend |

`Ctx::held` counts toward no limit (stated on the page): a word or two per element of a term the
process holds or is given, gone with the call. `list_arg` now takes `&mut Ctx`: helpers `objects`
(ets), `float_text`, `iovec`, `monitor_options`, and `re`'s `spec`/`run_options` follow.

Every native that asks does so before any side effect: `take` before removing, `erase/0` before
emptying, `select_delete`/`select_replace` ask only inside `select_all`, before they change the
table; `list_arg` is called before side effects at all 36 sites (checked).

## Tests (host, `tests/allocation.rs`, fixture `tests/src/allocation.erl`)

16 new, 27 in the file. The fixture's .beam rebuilt with the pinned OTP 28.5 erlc
(`erlc +deterministic`). The test allocator gained one hook: a process displaying
`{pages, KiB}` limits its thread to KiB more from then on (used by the list_arg test only, since a
native's copy of a list is half the list's own cells and cannot pass the 1 MiB single-allocation
cap before the list does); `run` resets it.

Red on main: each new test run alone at 328bcecff in a scratch worktree (.tmp/B107/base, now
removed), the new allocation.rs and fixture copied in: all 16 abort the test binary, "memory
allocation of N bytes failed" (SIGABRT, cargo exit 101): maps keys/values/pairs/from_list 1048592,
from_keys 1048656, the four ETS 1836544, characters 1600000, decoded 1048608, the three dictionary
1048608, bignum digits 1048608, list copy 65536. Record: .tmp/B107/red-on-main.txt (dict_keys's
first shape, 15,000 keys made 20 times, was killed at a safe point on main, not red; reshaped to
20,000 keys made twice, red). Two fixture traps: erlc folds `integer_to_list(1 bsl 200000)` (and
`0 * unique_integer()`) to a literal; the width is read from the dictionary.

## Gates (exit codes)

- `q run --cores 4 -- cargo test -p beamlet-vm --test allocation --release`: 27/27, exit 0.
- `q run --cores 6 -- cargo test --workspace --release` (userland/otp): all pass, exit 0;
  `-p beamlet-vm -p beamlet-re` rerun after the last comment edits: exit 0.
- `cargo check --workspace --all-targets`: clean. clippy -p beamlet-vm -p beamlet-re
  --all-targets: no warning in a changed file (11 pre-existing in binary.rs, phash.rs, interp.rs,
  loader.rs, vm.rs, untouched).
- `cargo build -p beamlet-redoubt --release` rv32imac and rv64imac: exit 0 both.
- `make -k -f scripts/jobs.mk set CASES="docs formatting"`: PASS both (5.5 s, 28.6 s), exit 0, at
  head 9b1903341.
- `set CASES="beamlet-lookup-host beamlet-lookup-cli-host"`: PASS both, exit 0.
- `q run --cores 8 -- ./test-shell`: exit 0, "every stage passed" (beamlet 384 passed 1 skipped;
  BEAM 315 passed 70 skipped). Log .tmp/B107/test-shell.log.
- `q run --cores 8 -- tools/difftest erlang atomvm`: 512/512, 21 skipped by design, exit 0;
  `tools/difftest elixir`: 8/8, exit 0.
- Not run: any boot case. B103 holds the quiet slot and the brief says ask first. Proposed
  regression set, both widths: beamlet-heap-allocation, beamlet-budget-flood (unchanged:
  term_to_binary's buffer is not converted, so its module's comment holds), beamlet-natives,
  beamlet-footprint (8 jobs; or `scripts/shell-cases`, the whole beamlet set, as the train will).
- No new boot case: none of the converted natives was shown to reach a size the VM's pages refuse
  under the process's limit in a boot; establishing one needs boots on main and the branch.

## Pages

docs/userland/beamlet.md, "Limits inside one VM": tested (55) -> (71), the 16 tests listed; the
"One allocation" bullet names the converted natives and binary_to_term's steps, and a sentence
on a native's copy held aside (allocator only, no limit); the backstop paragraph narrowed to: a
native's own allocation (zlib, crypto, term_to_binary's encoding, bignum arithmetic and a decoded
bignum's digits, counted but taken as they come, an ETS insert's copy of its object, the
dictionary's own table), a native not named that builds more than the headroom (maps:merge/2, a
match specification's body), a message's copy into its receiver.

Summaries checked, no change needed: docs/userland/shell.md 425-432 (refers to the checked
allocations by beamlet.md's list, still true); docs/plan/m2-usable-shell.md 35-38 (a runaway
allocation ended by the process's heap limit, the session's together by its budget: still true);
the compile residual, beamlet.md "a growth the check does not see" (still true); README.md,
GETTING-STARTED.md, userland/otp/README.md, userland/otp/DESIGN.md, docs/SECURITY.md (no claim on
allocation failure or these limits); docs/userland/agents.md 307 (binary_to_term not used, unaffected).

## Open

- Residuals now named on the page and not converted: maps:merge/2 (a put per entry, garbage
  between), a match spec body's results, ETS insert's own copy (made before its limit check),
  the dictionary's entries Vec, a decoded bignum's digits, a decoded fun's cell temp.
- Read in full: the diff, the new/changed regions with their context, and allocation.rs,
  allocation.erl, maps.rs, map.rs, copy.rs's OwnedTerm; the unchanged remainder of the large files
  (ets.rs, proc.rs, erlang.rs, etf.rs, bif/mod.rs, process.rs, term/mod.rs, re/lib.rs, 9.5k lines)
  was not read line by line.
- `Heap::map_from` now sorts and builds a balanced tree for every caller (loader literals, map
  literals in natives): same maps, a different tree shape; nothing reads the shape (equality,
  order, iteration, encoding all go by key order); difftest and test-shell agree.

## Round 1 (servers-red BLOCK), folded: head 207cce168

Commits: c24515279 (beamlet + tests), 207cce168 (docs); base 328bcecff.

- P1: `integer_to_list`/`integer_to_binary` now ask for the most digits first (exact `ceil(bits /
  log2 r)` for a power of two; else `k * ceil(bits / ilog2(r^k))`, r^k the largest power in a word,
  at most 1.6% over), then copy the magnitude as words (held, no BigInt clone) and make the digits
  in place: power-of-two radixes read off the bits (linear), others divided out k at a time by r^k.
  No String, no uppercase copy. `integer_to_binary` uses `Ctx::buffer`. Unit test
  `digits_are_the_text_in_every_radix_within_their_bound` (radix 2..36 against num-bigint's text,
  zero to 4,099-bit and 9-word values, and the bound). Host test
  `a_bignums_binary_digits_past_the_allocator_kill_their_maker`: `integer_to_list(1 bsl 16777000,
  2)` under a 4 MiB single-allocation cap (thread-local `ONE`, so the 2 MiB bignum exists); red on
  main ("memory allocation of 16777001 bytes failed"). Text-producing audit: characters_to_list
  (done in round 0); characters_to_binary's text named as a residual; float/term printing not
  touched.
- P2 converted: lookup_element's elements copy (held), chunk() (lists built from the results
  where they are, no split_off/drain), the tuple natives' copy (`tuple()` held: setelement,
  append_element, insert_element, tuple_to_list), apply/2,3's argument list (refused past 255
  before more are copied, `Heap::to_vec_at_most`), and the natives' direct `Heap::to_vec` copies
  made `list_arg` (binary:matches/split patterns, longest_common_prefix/suffix, process_info list,
  ensure_modules_loaded, the info at info.rs:998) or `list_len`/`is_proper_list` (halt, link/2).
  Tests (pages hook, 64 KiB): lookup_element over 5,000 objects, match_object/3 chunks of 4,000,
  setelement on a 30,000-tuple; all red on main (131072, 80000, 480000 bytes).
  Named precisely on the page (no "such as"): term comparison's work list and two maps' entries,
  phash2's and the printer's map entries, flat_size's work list, error/2,3's argument and location
  copies, open_port/2's option/args/env lists, update_counter/update_element's element copy, a
  match spec's guard/body and evaluated-tuple copies, characters_to_binary's text, an unaligned
  binary copied before characters_to_list or binary_to_term reads it; term_to_binary's work list
  and map entries under its encoding; heap growth in natives not named (maps:merge's puts, match
  spec results); a message's copy.
- Page: tested (76), +5 entries; One allocation bullet adds the integer text; the held sentence
  names tuples, lookup_element's elements and a bignum's magnitude.

Gates at 207cce168 (exit 0 each): allocation 31/31 and otp workspace tests (release); clippy no new
warning (interp.rs:1499 is the pre-existing 1502 moved); `set CASES="docs formatting
beamlet-lookup-host beamlet-lookup-cli-host"` 4 PASS; difftest erlang atomvm elixir 520/520;
./test-shell every stage passed; prebuilt + `set CASES="beamlet-heap-allocation
beamlet-budget-flood beamlet-natives beamlet-footprint"` 8/8 PASS (heap-allocation rv32 15.3 s,
rv64 17.8 s; budget-flood 0.7/0.7; natives 2.2/2.2; footprint 5.3/5.5), log .tmp/B107/boots.log.
Host work pinned 8-19 after the correction.

## Round 2 (servers-red OK with notes), folded: head ae4568494

Commits: 4f218c498 (beamlet + tests), ae4568494 (docs); base 328bcecff.

- Borrowed instead of copied (aligned binaries; an unaligned one is still copied by `to_bytes`):
  binary:copy/2's source, binary:decode_unsigned, binary:longest_common_prefix/suffix (bytes_of
  dropped there), and on the same pattern binary_to_integer, binary_to_float (`to_string` gone)
  and erlang:display_string of a binary (`into_owned` gone).
- Named on the page: decode_unsigned's bignum (a native's own); the pattern copies
  binary:compile_pattern/1, match/2,3, matches/2,3 and split/2,3 (and so replace, which OTP's
  binary.erl implements with match/matches) keep in their matcher; display_string's text of a
  list; and, generalised, a binary off a byte boundary copied whole by any native reading its
  bytes.
- Gates at ae4568494 (exit 0): docs PASS, formatting PASS; otp workspace tests (allocation 31/31);
  difftest erlang atomvm elixir 520/520; prebuilt + beamlet-natives rv32 PASS 2.1 s, rv64 PASS
  2.1 s.
