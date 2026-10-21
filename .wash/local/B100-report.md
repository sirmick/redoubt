# B100 report: the 9P mux takes a send's messages where they lie

Branch wp-B100 in .worktrees/B100. Head 85f169765: two commits on wp-B102 (25ba031c7; rebased
there for train 91). Originally based on 820d5e5ff, head a44fae697 at the first review.
Tier A (libs/rt serving library). Shape agreed with the Architect on QA thread B100-shape (option B,
four conditions; then (a) for bootfsd's stack).

- f15d2c22b `rt: a 9P mux send's messages are taken where they lie, never listed`
- 85f169765 `testbench: a session packs one 9P mux send at bootfsd`

## What changed

- `libs/rt/src/server/ninep_mux.rs`: `stored()`, which built a `Vec<(Stored, usize)>` of every
  message in a send before admission, is gone. `framed()` walks the transfer once and lists nothing:
  it frames every message (a message that does not frame still ends the session before any is
  taken, as before) and notes whether any is too long for the words. `take_all()` then walks the
  transfer again and takes each message straight into `take()`, stopping where the session ends.
  The pages are admitted once if any long message lives in them, and released at the end if no
  request holds them. Order, admission, the page charge, the wire and the public API (`deliver`,
  `take_request`) are unchanged. The comment on `framed` says why there are two walks (condition 1).
  rt's code lines: unchanged at the size budget's 4,118 ceiling (the first version was 5 over; I
  trimmed it, so there is no raise).
- Why not cap at MAX_TAGS: a `Tflush` frees its oldtag as it is taken, so a well-formed send can
  carry more than 256 messages (a chain of Tflushes freeing each other's tags). A cap would be a
  new protocol rule.
- `image/manifest.json`: bootfsd `stack_pages` 4 → 5 (Architect's (a)).

## Tests

Host (libs/rt/tests/mux.rs, with a per-thread counting global allocator; the server thread's heap
is published at each turn):

- `a_sends_messages_cost_the_servers_heap_nothing_each`: the server thread's heap over three
  sends must stay under one page. Bytes over the send:

  | Send | 820d5e5ff | head |
  | --- | ---: | ---: |
  | 9,362 7-byte messages (MAX_LEND_PAGES), tags i % 256 | 983,164 | 988 |
  | a full transfer of Tflushes freeing each other's tags (legal, 7,281) | 491,560 | 40 |
  | a page of 74 writes too long for the words (long path) | 7,720 | 40 |

  The test fails on 820d5e5ff (measured in a scratch worktree with the test copied in).
- `a_send_that_ends_its_session_part_way_leaves_nothing_held` (condition 2): a page of writes ending
  its session at a reused tag part way, and one whose last message does not frame. Each time the
  parked call is answered ENDED, nothing stays held, and a page of two writes is served after. It
  passes on base too; it guards the two-pass ordering.

Attack case `tests/session-mux-flood.toml` (both widths, image/boot.toml, `memory = true`):

- A session reaches the raw send itself. beamlet's `send` native takes any words and up to
  MAX_BUFFER (65,536) bytes. Alice's session, in Elixir alone, mints its own connection at bootfsd
  (`NinepCommon.new_connection`, so the mux session is not her VM's) and opens a session. It sends
  9,362 7-byte messages in 65,534 bytes; the 257th is a tag in use. A second completion call gets
  OPENED (bootfsd's word that the first ended). Then it disconnects, and the session reads /boot on.
- Verdict (rule F): the bench's RAM scan of bootfsd's own heap record, peak ≤ half its cap.
- Red on 820d5e5ff, rv32: `FAIL  session-mux-flood [rv32, smp=1]  bootfsd: heap needs 2670 pages
  for twice its 1335-page peak, capped at 2510`. Head: 1,147 pages, PASS.
- rv64 is no witness: the cap is sized from rv32's larger boot peak, so base 1,177 against head 942
  fits under the cap. The case description and serving.md say rv32 is the width that can fail
  (condition 3).

## bootfsd's stack (Architect's (a))

The case is the first memory case to drive a session's mux traffic and `new_connection` at
bootfsd. On rv64 bootfsd's first stack peaked at 8,952 bytes (9,112 on base: pre-existing), over
half of 4 pages. The image now declares 5, twice 8,952 rounded up. testbench.md's memory table row
reads 8,952 | 5, with its source in the prose. Measured on the head:

- session-mux-flood: rv64 8,952, rv32 7,336.
- userland-boot, init-boot, userland-read-only, steward-session-ends, steward-ssh-two-principals:
  7,256 (rv64), 5,752 (rv32).

Other manifests declaring bootfsd keep 4, since no memory case of theirs reaches this path.
beamlet-footprint and beamlet-compile-footprint (their own manifests) peak at 6,056 on rv64. The
other ~45 test manifests run no memory case.

## Gates (exit codes)

On add43455a, with the same code and manifest as the head (the head differs only by testbench.md's
memory status line naming the case, and the case file's comment):

- `prebuilt`: 0.
- `rv64/size-budget rv64/docs rv64/formatting rv64/rt-host-tests`: 0, all PASS.
- Earlier on 46c256fdc (same code): `rt-host-tests client-host-tests ipd-host-tests
  sshd-host-tests`: 0, PASS; `consrelay-host-tests rt-miri formatting`: PASS (rt-miri 86 s).
  size-budget then FAILED at 4,123/4,118, fixed by the trim and PASS above.
- `set CASES=session-mux-flood`: 0, PASS rv32 and rv64.
- `set CASES="beamlet-files aio-many-reads"`, `"aio-many-reads-two userland-boot"`, `"init-boot
  userland-read-only"`, `"steward-session-ends steward-ssh-two-principals"`, `"beamlet-footprint
  beamlet-agent-footprint"`: all 0, all PASS, both widths.
- `beamlet-compile-footprint`: PASS both widths.
- On head a44fae697 (final): `prebuilt` 0; `set CASES="walfsd-pools session-mux-flood"` 0, all PASS
  both widths (bootfsd: rv32 heap 1,147 / stack 7,336; rv64 heap 942 / stack 8,952 of 5 pages);
  `rv64/docs rv64/formatting` 0, PASS.
- Load runs: none. Every set was at most 4 jobs. I stopped my jobs at once at each quiet slot (my
  prebuilt in the first; walfsd-pools and the final run in the second and third).
- Consumer sweep: not needed. libs/rt's public API and contract are unchanged (deliver and
  take_request keep their signatures and behaviour). rt-host-tests, client, ipd, sshd and consrelay
  host tests pass, and beamlet-files and aio-many-reads cover beamlet's hub against real servers.
- rv32 builds: every rv32 boot case above built and booted.

## Pages and summaries checked

- Changed: docs/servers/serving.md ('A request': taken in place, never listed; any session can pack
  such a send through `send`, so the bound is the server's to keep; rv32 the witness. The
  Multiplexed connections and R77 status lists gain the two host tests and the case; R77's text gains
  the heap claim.)
- Changed: docs/servers/bootfsd.md (the listing residual is gone; one sentence that a send adds
  nothing past its requests), docs/SECURITY.md (R77 row: property and Tested-by, equal to R77's status
  set), docs/testbench.md (memory status line, bootfsd row and its source), docs/plan/m3-agents.md
  (attack row "Packed requests").
- Checked, no change needed: README.md, GETTING-STARTED.md, docs/servers/README.md and
  image/README.md (no mux or bootfsd stack claim); docs/userland/native.md's hub section (frames its
  own requests, a page a send: still true); docs/userland/beamlet.md's `send/2` (true as written);
  the M3 progress paragraph (no claim about this).

## Fold round 1 (servers-red P2)

- session-mux-flood retries its `:redoubt.send` for 2 s in 10 ms sleeps, as B102's
  steward-context-login does, prints `{:ok, tries}`, and bounds the expect `within = 60`. Its forbid
  is `\{:flood, [^}]*\{:error`: the letter's `\{:flood, .*\{:error` failed both widths, because
  the shell redraws the typed line in pieces with no newline and its echo holds both strings
  (checked against those transcripts, kept in .tmp/B100/fold1/).
- Rebased onto wp-B102. steward-context-login's comment now names both callers of
  `:redoubt.send`.
- Gate on ab2a74aac, all exit 0: prebuilt; session-mux-flood and steward-context-login PASS on
  rv32 and rv64 (bootfsd heap 1,147 / 942, stack 7,336 / 8,952 of 5 pages); docs PASS;
  formatting PASS.

## Train 91 fix (init-host-tests)

- Train 91 failed `the_boot_profiles_unverified_copies_are_the_image_less_its_verification`:
  tests/data/boot-profile/manifest-unverified.json must equal the image's manifest less its
  verification, and its bootfsd kept 4 stack pages. It now takes 5. This is the one exception to
  "other manifests keep 4": a test holds it to the image. No other test ties a manifest to the
  image's (steward.rs, sshd/slot.rs and testbench read the image itself).
- Gate on 85f169765, all exit 0: prebuilt; boot-profile-unverified PASS rv32 and rv64;
  init-host-tests PASS (the test itself checked by name: ok); docs PASS.

## Observations (not changed)

1. beamlet on rv64: `for i <- 0..9361, into: <<>>, do: <<...>>` typed at the shell ended alice's VM
   with `memory allocation of 3269248 bytes failed` (a Rust panic, the lease exited 101), not an
   Erlang error. rv32 passed the same line. It is inside the session's own budget, but a VM that
   panics on an allocation an expression asked for is worth a look (Tier B, beamlet). Logs:
   .tmp/B100/run1/.
2. docs/testbench.md "Which cases run in guest time" counts (145 of 226, 38, 8, 68) are long stale:
   the tree has 351 boot cases, 187 under icount. No checker holds them, so I did not touch them.
3. Stopping a bench run mid-fingerprint left a stale `.git/worktrees/B100/index.lock` (no git
   process). I removed it once I had confirmed that.

## Next

Servers-red review of 978bfbe16..a44fae697.
