# BEAM17 checkpoint: sized resources in ETS and persistent_term

wp-BEAM17 at main 38925184e; nothing written yet.

## Facts (two change the premise)

1. On Redoubt `max_ets_words` is not 1 GiB. The platform sets it, as it sets `max_heap_words`, to a
   sixteenth of the VM's budget (userland/otp/redoubt/src/lib.rs:608-620): about 2.8 MB for a
   session's 11,008 pages. The 1 GiB (2^27 words) is the host CLI's default, where there is no
   budget.
2. ETS already counts declared bytes, at insert. An object is an `OwnedTerm`, and
   `OwnedTerm::words()` includes its heap's `held_bytes` (SHELL3), so `ets::weigh` counts an atomics
   array or a zlib stream at its size then. The gap: a stream grown after insert (enqueue grows its
   queue) keeps its insert-time weight. Each growth is bounded by the grower's own heap limit, but
   any number of streams can each carry one.
3. `persistent_term` has no size limit at all. `MAX_PERSISTENT_TERMS` (65,536) counts terms, not
   words, and a replaced value is never freed. So one process can park 65,536 x 128 MB atomics
   there. This is the real hole.

## Proposal

- persistent_term: a new `Limits::max_persistent_words`, counting each put's words (declared bytes
  included), cumulative since a replaced value stays. A put past it is `system_limit`, as for ETS.
  On Redoubt the same sixteenth share; the host default 2^27, as for ETS.
- ETS's resize staleness, one of:
  (a) recommended: ETS keeps the resources each object holds and re-reads their declared sizes
      when it checks room, so a grown stream counts at its live size. The cost per insert follows
      the resources ETS holds, which the limit bounds.
  (b) Refuse a resizable resource (zlib stream, screen buffer) in ETS or persistent_term as
      `badarg`: OTP never stores either there, but it departs from BEAM.
  (c) Leave it as a stated residual.
- Footprint: counting changes no memory, only what a limit refuses. I will measure the shell's ETS
  and persistent_term words at its prompt (report_memory) and confirm both are far under the
  share. Its own use is small (about 6 call sites, plus OTP's logger and application env).
  beamlet-footprint is in the gates.

## Tests

VM host tests: a `persistent_term` put of a large atomics array past `max_persistent_words` is
`system_limit`; with (a), a zlib stream grown after an ETS insert makes a later insert past the
limit `system_limit`.
