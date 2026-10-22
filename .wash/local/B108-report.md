# B108 report: an_admission_refusal_of_the_moment_is_tried_again

Base main 328bcecff; head wp-B108 047dd3432 (one commit, `libs/client/tests/file.rs` only).

## The interleaving

The fake kernel logs each call at entry (`libs/rt/fake/src/lib.rs` `Transport::call`). The test
waited for `calls(client).len() >= 4` to mean "the holder holds the working fid", but
`Connection::attach` alone makes four: `map_anon` (the lend), `call` (Tversion), `call` (Tattach),
`unmap` (the lend dropped). The wait passed at once, and the second open raced the holder's.

Traced (temporary eprintln of `fake().calls(client)`, not committed):

- Usual run (alone, 3/3): at main's walk the log is `[map_anon, call, call, unmap, map_anon]`; at
  main's open served it is `[..., map_anon, map_anon, call, call, call, receive]`: main's walk
  won the fid; the holder's walk was refused and waited (`call`, `receive`); main's open served.
  Main was never refused; `> before + 2` passed on the holder's calls.
- Failing run (5/240 under 4 parallel copies on cores 8-23): at main's open served the log is
  `[map_anon, call, call, unmap, map_anon, call, call]`: the holder's thread had made no call; main
  walked and opened unrefused; exactly `before + 2`; failed in ~0.1 s, the signature of B106's and
  train 97's failures (0.07-0.08 s). Logs: .tmp/B108/nat-{2-1,1-20,3-1,3-5,3-57}.log.
- A second, rarer way: main stalled >= 50 ms after the wait lets the holder's 50 ms sleep end
  first (forced with a 60 ms stall: fails the same way).

Not the client's bug: the admission rule and the retry behaved as written in every trace.

## The fix

The holder sends the log length over a channel once its open is answered (its fid is held at
the server), and closes only once a `receive` (the admitted() wait after a refused walk) appears
in the log after that point; the holder makes no call meanwhile, so that `receive` is the other
open's. Main takes `before` from the channel and asserts a `receive` since. The 50 ms sleep is
gone. The second half (held past the bound: 1 + 2*8 calls) is unchanged.

## Gates and runs (all exit 0 unless said)

- `q run --cores 4 -- cargo test -p redoubt-client --test file --no-run`: built.
- fixed binary alone, pinned 8-23: 3/3, 10 passed, 2.57 s.
- mutation: `admitted` with `for _ in 0..0` (no retry): the test fails (main's open unwrap,
  too_many), 0.00 s; reverted.
- `rustfmt +nightly --check --edition 2024 libs/client/tests/file.rs`: clean. The
  `jobs.mk set CASES=formatting` bench case was not run.
- 4-way load, cores 8-23: old binary 5/240 failed; fixed binary 240/240.
- Witness (orchestrator's go, cores 8-19, nice 10): 50 runs of the fixed binary, 8 at a time,
  beside 12 looping copies: 50/50 `10 passed`, 0 failures, 2.57-2.60 s each. Logs
  .tmp/B108/wit-*.log.
- Not run: `jobs.mk set CASES=client-host-tests`, docs case (no docs change).

## Summaries checked

- docs/servers/serving.md:732 and docs/SECURITY.md:139 cite the test by name; name and claim
  (a refusal of the moment is tried again within the bound) unchanged: no update.
- libs/client/src/file.rs module doc (retry rule): unchanged code, still accurate.
- No README/GETTING-STARTED/milestone claim concerns this test.
