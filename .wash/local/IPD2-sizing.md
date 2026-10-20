# IPD2 sizing checkpoint (thread IPD2-sizing)

Branch wp-IPD2 from 453a48bfc, code uncommitted at the time of writing (host tests green:
`cargo test -p redoubt-rt -p redoubt-ipd -p redoubt-consoled`, exit 0; guest builds and the
other consumers running).

## The mechanism as built

- `libs/rt` `Override` gains `account: u64`, keyed as `AdmitKey` is: `(0, root badge)` as today,
  or `(account != 0, badge 0)` for that account's **unlabelled** bucket. `Override::covers(key)`
  is the one matching rule (the admission's `cap` and ipd's `SocketCaps::of` both use it).
  `with_overrides` refuses an override that is neither form, or names a bucket twice; the
  worst-case rule is unchanged (each override slot at max(its cap, default)).
  `AdmitKey::root()` lost its last caller (`SocketCaps::of`) and is deleted.
- ipd: `account=ACCOUNT:INFLIGHT:STATE:SOCKETS`, parsed as `limits=` (fields <= 64), account
  non-zero, once, at most 8 beside at most 8 `limits=`; `Limit` gains `account` and sizing turns
  both into `Override`s uniformly.
- Consumers whose `Override` literals gained `account: 0`: consoled's `init_override`, the rt
  fuzz target, ipd's fake world.

## The arithmetic (asserted in servers/ipd/tests/sizing.rs, `the_images_account_override_fits`)

- Default bucket: 712,704 B. alice's at 5 / 4 / 32: fids `files_for(32)` = min(72, 64) = 64,
  `State` 4 + 32 = 36, so 40 fids x 512 + 24 units x 17,408 more = 1,150,976 B.
- Image need: 6 x 712,704 + 1,150,976 = **5,427,200 B = 1,325 pages** (was 7 defaults
  4,988,928 = 1,218). With heap cap 74 + stack 128: 1,527 of the entry's 4,096; the budget
  (3,894 pages) holds it.
- `max_sockets`: 6 x 12 + 36 = **108** (was 84).
- Fids bind alice's **live** sockets: (64 - 2 roots) / 3 shares = 20 working fids per session,
  2 per live socket. A TIME-WAIT socket holds no fid, only its unit.
- Her SSH session's share beside her console session (each session's `/net` is one working
  `State` unit in its own share, settled at login when the namespace attaches):
  floor(36 / 3) = 12, less its connection = **11 sockets** (was floor(12 / 3) - 1 = 3).

## Finding: the heap cap does not hold the sockets, today or after

ipd.md says the heap cap (74 pages) holds "the stack's tables and every socket's buffers".
It does not:

- A socket's buffers are two `vec![0; 8192]`, each a large heap block of exactly 2 pages, plus
  smoltcp's storage, 432 B (measured, `size_of::<SocketStorage>()`, host 64-bit). That's about 4.1
  pages a socket, inside `SOCKET_BYTES` (17,408 B = 4.25 pages).
- **Today:** 84 sockets x 17,408 = 1,462,272 B = **357 pages**, against a cap of 74 with an idle
  peak of 37 (testbench.md memory table). Room above the idle peak: 37 pages = 151,552 B =
  **8 sockets**. Past that, the allocation fails in the runtime and `ipd` panics (no fallible
  path in `Stack::new_socket`). The image's own default caps reach past 8: alice's two
  sessions at 3 each, plus bob's lone session at floor(12 / 2) - 1 = 5, gives 11. This is
  derived, not booted.
- **After:** 108 x 17,408 = 1,880,064 B = **459 pages**. alice's SSH session alone (11 sockets,
  46.75 pages) passes the 37 pages of room, so the new case would crash ipd at the old cap.
- The budget term already counts each socket's `State` unit at `SOCKET_BYTES`, but outside the
  heap, where the buffers do not live.

## Fix options (a ruling is needed: scope reaches beyond ipd's image entry)

1. **(Recommended) The heap cap holds every socket, checked at start.** ipd's sizing refuses to
   start unless heap_pages x page >= base + max_sockets x SOCKET_BYTES, where the base is the
   measured working set at twice its peak (74). Image: 74 + 459 = **533**, and the entry stays at
   4,096 (1,325 + 533 + 128 = 1,986). Cost: every manifest running ipd (35 under tests/data plus
   the image) gets its `heap_pages` derived the same way. That's a mechanical edit, but outside
   my owned paths, and the bench's `every_manifest_entry_holds_its_buckets_at_their_caps` grows
   the heap term. The budget term's double count of socket bytes stays, as a residual with a
   follow-up.
2. **The same, but only the image's heap raised (533), with no start check.** The rule is
   stated on ipd.md and the other manifests keep 74. It's cheaper, but the claim stays
   unenforced and test manifests keep the crash path.
3. **Additionally (with 1 or 2):** make `new_socket` fallible (`try_reserve` the buffers), so a
   heap gap is refused `too_many` and never panics. This is defence in depth, not a substitute:
   a full heap would then be a cross-account channel.

Recommendation: 1 + 3. I'll hold the case until the ruling.
