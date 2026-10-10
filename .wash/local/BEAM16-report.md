# BEAM16 report: atomics and zlib streams count toward their holder's heap limit

Branch wp-BEAM16, worktree /home/mcloonan/redoubt/.worktrees/BEAM16, on main 2e3057e64. One commit,
3053aae86. Tier A (the VM); the beamlet red reviews.

## What changed

- userland/otp/vm/src/bif/atomics.rs: `new` makes the array with `c.new_resource_sized(a, n * 8)`
  (its cells, 8 bytes each; up to 2^24 cells, 128 MiB). The size never changes.
- userland/otp/vm/src/bif/zlib.rs: `held(s)` = queued input + output not handed out + the codec's
  state; `counted(c, t, s)` declares it with `Ctx::resize_resource`. Called after `close`,
  `deflate_init`, `inflate_init`, `enqueue`, `reset`, `deflate_end`, `inflate_end`, and after
  `deflate` and `inflate`, whatever they return: their bodies moved into `deflate_on` and
  `inflate_on`, so an error after input was drained cannot leave a stale count. `open` declares 0
  (`c.new_resource`).
- Codec state: a deflater is `size_of::<CompressorOxide>()` plus `DEFLATE_BOXED`, the tables
  miniz_oxide keeps behind its own boxes: dictionary 32,768 + 258, two hash chains of 32,768 u16s,
  Huffman tables 3 x 288 x (2 + 2 + 1): 168,418 bytes in the vendored 0.9.1. Those constants are
  pub(crate) in miniz_oxide, so the number is a literal, its source named in the comment. An
  inflater is `size_of::<InflateState>()` (its dictionary is inline) plus its gzip frame bytes.
- docs/userland/beamlet.md, "Limits inside one VM": the Process memory bullet says what counts (a
  screen buffer, an atomics or counters array's cells, a zlib stream's queues and codec, on each
  process heap that holds it) and what does not (a resource in a message not yet received, in ETS
  or in persistent_term; another holder counts a resize from its next collection); an array or a
  stream put in ETS or persistent_term and dropped by its holder is bounded only by the budget.
  Status 17 -> 23 with the new tests. The Compression bullet says a stream's memory counts toward
  its holder's limit.

## Tests (vm/tests/sized.rs, fixture vm/tests/src/sized.erl, rebuilt with the pinned erlc)

Under a limit of 100,000 words (800 kB), calling the natives directly (no OTP modules loaded):
- an_atomics_array_within_the_limit_is_held: 1,000 cells -> normal.
- an_atomics_array_past_a_processs_own_heap_limit_ends_it: 1,000,000 cells (8 MB) -> killed.
- counters_arrays_past_the_limit_together_end_their_holder: 100 x 10,000 cells -> killed.
- a_zlib_stream_within_the_limit_is_held: 1 kB queued -> normal.
- a_zlib_streams_queue_past_the_limit_ends_its_holder: 10 MB queued; the queued binary itself is
  off the heap and not counted (max_heap_size excludes shared binaries) -> killed.
- zlib_codecs_past_the_limit_together_end_their_holder: six deflaters, nothing queued -> killed.
Against the old natives (copied back from HEAD, restored afterwards; no stash): the four "past"
tests FAIL, the "held" ones pass. Counting only the deflater's inline part, the six-codec test
FAILS too (about 66 kB each, under the limit), so it pins the boxed part.

## Gates (all through q; scratch /home/mcloonan/redoubt/.tmp/BEAM16)

- `cargo test -p beamlet-vm` (userland/otp): rc 0, 90 passed, 0 failed.
- `./test-shell`: rc 0, every stage passed.
- `tools/difftest` (all suites): 517/523, 6 failed, 18 skipped by design. All six are in the
  atomvm suite (test_binary_to_term, test_code_all_available_loaded, test_code_server_nifs,
  test_display_string, test_node, test_unicode), none about atomics or zlib. With the old natives
  the atomvm suite fails the same six (457/463), so they are main's, not this change's. Earlier
  packages ran `tools/difftest erlang` only, which is why they were not seen.
- prebuilt rv64 233 / rv32 219, 0 failed; PASS rv64 and rv32 beamlet-files and beamlet-boot; PASS
  docs, formatting, unsafe-budget (no unsafe added).

## Open risks

- A sized resource in ETS, persistent_term or a queued message counts toward nobody (the red's
  note on SHELL3, now on the page). For atomics and zlib that leaves a process able to create
  arrays or streams, store them in ETS or persistent_term and drop its own references: bounded only
  by the session budget. Closing it would mean counting sized resources in ETS's words
  (max_ets_words) and in persistent_term; not done here.
- DEFLATE_BOXED is a literal from the vendored miniz_oxide's private constants; an upgrade that
  changes them needs it redone (named in the comment).
- The atomvm difftest suite's six failures on main: worth a node.

## Summaries checked

beamlet.md "What runs on it" status (mentions the zlib bound not attacked by a named test: that is
MAX_QUEUED's system_limit, still unattacked, unchanged); docs/SECURITY.md (no row on VM heap limits:
unchanged); docs/plan/m2-usable-shell.md (no claim about atomics or zlib: unchanged).

## Red BLOCK folded (head 5ce5d20cc, amended over 3053aae86)

- P1, the stash: `held()` now counts the stashed term (`OwnedTerm::words() * 8`), and
  `set_stash` and `clear_stash` declare the new size. Test
  `a_zlib_streams_stash_past_the_limit_ends_its_holder`: an unlimited process builds a list of
  200,000 integers (3.2 MB), stashes it in a stream and sends the stream to a process limited to
  100,000 words, which is killed. With the stash uncounted it FAILS (the limited process ends
  normally).
- P2, the output buffer: the earlier figure, 168,418 bytes, missed `ParamsOxide.local_buf`
  (`Box<LocalBuf>`, `OUT_BUF_SIZE` = 64 KiB x 13 / 10 = 85,196 bytes). DEFLATE_BOXED is now
  253,614 bytes, and is `pub` (re-exported as `bif::DEFLATE_BOXED`). New
  `vm/tests/zlib_codec.rs`: under a `#[global_allocator]` that counts this thread's allocations,
  `CompressorOxide::new` allocates exactly DEFLATE_BOXED, and `InflateState::new_boxed` exactly
  `size_of::<InflateState>()`; both pass, so a miniz_oxide that changes its buffers fails this
  test, not the limit. The test's allocator is the one `unsafe` it needs (a test crate, outside the
  unsafe budget; unsafe-budget and no-cruft pass).
- The page's zlib wording names the stash and the measured codec state; status 23 -> 26.

Gates on the head (q): beamlet-vm 93/0; ./test-shell every stage passed; difftest (all suites)
517/523, the same 6 atomvm failures, main's (B34); prebuilt 233/219 0 failed; PASS beamlet-files
rv64 and rv32, docs, formatting (after rustfmt put the new `pub use` beside `pub use proc::send`),
unsafe-budget, no-cruft.
