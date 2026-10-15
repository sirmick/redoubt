# B65 report (b51-implementer, 2026-10-15)

Node B65: a fun typed at the shell prompt pays ~0.18 µs per byte of every binary among its
bindings on each call (B60's measurement). Worktree `/home/mcloonan/redoubt/.worktrees/B65`,
branch `wp-B65` from main 1dd0acb27. Phase 1: investigation on the host; no code changed yet.

## Phase 1: where the per-byte cost arises

**Answer: beamlet's term comparison. erl_eval merges a fun's bindings on every call and, for a
variable bound in both sets, tests the two values with `=:=`; the two are the same term, and the
BEAM's `eq` answers "same term" without reading a byte, but beamlet's `eq_exact` memcmps the
whole binary.** The BEAM shows no slope; beamlet alone does: the VM's, so phase 2 is beamlet's.

### The harness

`$REDOUBT_TMP/B65/b65.erl` (Erlang, `erl_eval` directly) and `b65_ex.ex` (Elixir,
`Code.eval_string`, as the prompt runs), not committed. Each variant times 2,048 steps against a
binary of 4 KiB, 16 KiB, 64 KiB and 256 KiB (then 1 and 4 MiB) and prints µs a step:

- `compiled`: `walk(<<_/utf8, R/binary>>)` compiled in the module (the control);
- `evald`: the same walk as an interpreted fun, the binary matched each call;
- `grapheme` (Elixir only): the walk by `String.next_grapheme`, as B60's prompt walk;
- `env`: an interpreted fun that only *holds* the binary in its environment (`Big` bound when the
  fun was evaluated) and matches nothing: the bindings' per-call cost alone;
- `envarg`: an interpreted fun given the binary as an argument each call, body `byte_size(B)`.

Run on the BEAM (OTP 28.5.0.6, the toolchain's `erl`/`elixir`) and on host beamlet built from
wp-B65 (`RE_ENGINE=rust tools/build-beamlet`, release; and a second build with line tables under
`$REDOUBT_TMP/B65/target-dbg` for the sampling), both with `-pa` to the toolchain's stdlib,
kernel, compiler and Elixir ebins.

### The numbers (µs a step; x86 host, release)

| variant | VM | 4 KiB | 16 KiB | 64 KiB | 256 KiB | 1 MiB | 4 MiB |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| compiled | BEAM | 0.010 | 0.024 | 0.008 | 0.009 | | |
| compiled | beamlet | 0.157 | 0.161 | 0.160 | 0.162 | | |
| evald | BEAM | 2.41 | 1.37 | 1.33 | 1.33 | | |
| evald | beamlet | 28.5 | 23.4 | 25.0 | 31.7 | | |
| env | BEAM | 0.58 | 0.38 | 0.35 | 0.35 | | |
| env | beamlet | 9.6 | 9.6 | 10.8 | 15.1 | 34.6 | 146.8 |
| envarg | BEAM | 0.17 | 0.15 | 0.17 | 0.15 | | |
| envarg | beamlet | 3.4 | 3.3 | 3.9 | 6.1 | 18.4 | 67.8 |
| evald (Elixir) | BEAM | 1.62 | 1.28 | 1.29 | 1.29 | | |
| evald (Elixir) | beamlet | 29.5 | 27.2 | 29.4 | 37.5 | | |
| grapheme (Elixir) | BEAM | 0.97 | 0.93 | 0.93 | 0.93 | | |
| grapheme (Elixir) | beamlet | 30.3 | 28.8 | 33.8 | 54.7 | | |

The BEAM is flat in the binary's size in every variant (erl_eval has no per-byte cost by
design). beamlet grows linearly: `env` +127 µs and `envarg` +58 µs from 1 to 4 MiB, i.e. about
30 µs a MiB a call on x86 — memcmp speed over the whole binary, once per interpreted call, in
a variant whose body never touches the bytes. Scaled to the machine (the constant 3.6 ms a call
B60 saw is ~120× the 30 µs here; a byte loop under TCG is slower still), that is B60's
0.18 µs a byte.

### The frame

The release binary is stripped and `perf` is closed (`perf_event_paranoid` 4), so the VM ran
under gdb (ptrace allows a descendant) and was interrupted eight times in a long `env` run over
4 MiB and eight in `envarg` (`$REDOUBT_TMP/B65/sample.sh`, `gdb.out`). Six of eight `env`
samples and eight of eight `envarg` samples were in the same stack:

```
__memcmp_avx2_movbe
core::slice::cmp  (cmp<u8>)
beamlet_vm::term::cmp::compare_bits      vm/src/term/cmp.rs:192
beamlet_vm::term::cmp::compare_one       vm/src/term/cmp.rs:169
Heap::cmp_exact / Heap::eq_exact         vm/src/term/cmp.rs:37, :57
interp::step  (op::IS_EQ_EXACT)          vm/src/interp.rs:1084
```

The remaining `env` samples were the collector (`maybe_collect`), as expected.

Who issues the `=:=`: erl_eval's `merge_bindings/4` (stdlib-7.3.0.2, erl_eval.erl:1951):

```erlang
merge_bindings(Bs1, Bs2, Anno, Ef) when is_map(Bs1), is_map(Bs2) ->
    maps:merge_with(fun (_K, V, V) -> V; (_K, _, V) -> apply_error(...) end, Bs2, Bs1);
```

The clause `(_K, V, V)` compiles to an `is_eq_exact` of the two values. erl_eval merges the
bindings after a body's expressions on every interpreted call (`exprs` → `merge_bindings`), and
a fun's environment binds every variable of the prompt's binding list, so the whole 256 KiB
`ascii` is compared with itself at every step of a walk — and B60's shell binds `ascii`,
`mixed`, `page` and `apage` at once, each paid for. On the BEAM `eq()` begins with
`if (a == b) return 1;`: the same term is equal without a scan, and a binary equal to itself is
the same term. beamlet's `eq_exact` (cmp.rs:42-60) short-circuits atoms, small integers, `[]`
and bitstrings of unequal length, then compares bytes; `compare` has no identity check either.

## Phase 3: rebased onto B60, shell.md's trap sentence as history (b65-implementer, 2026-10-15)

B60's final head named 94b26298b (on main 3d5464c41). `git rebase --onto 94b26298b 3d5464c41
wp-B65` (clean, 1/1). `docs/userland/shell.md` "The loop" step 3: B60's sentence ("costs about
3.6 ms plus 0.18 µs for every byte ... why is open") rewritten as history: a call costs the
evaluator's constant (~3.6 ms) whatever its bindings hold; it once cost 0.18 µs a byte more,
50 ms a step over 256 KiB, because the evaluator compares each binding with itself on every
call and the VM read the binary to answer; the VM now answers that a term equals itself without
reading it (link to beamlet.md#limits-inside-one-vm); the step no longer grows with the binary.
No package IDs. Amended into the one commit; the message's Page line names shell.md too.

**Head: wp-B65 = 43c42bbdc on wp-B60 94b26298b**, one commit, tree clean. Light gates at it (q,
4 cores): `cargo test -p beamlet-vm` 108 passed / 0 failed, `a_term_equal_to_itself_is_not_scanned
... ok` (0.12 s); `make -f scripts/jobs.mk docs` PASS 1.4 s rc=0; `cargo +nightly fmt -p
beamlet-vm --check` rc=0. Summaries grepped for the per-byte claim (README.md,
GETTING-STARTED.md, docs/README.md, beamlet.md): only shell.md:103 carried it; none other changes.

## Phase 2: built (Tier A, the orchestrator's word)

Review (steward-red-2): OK with notes; P3 folded: the comment states that one `Ptr` names one
object of one kind (what the cross-variant pointer compare rests on) and a `debug_assert_eq!` on
the two terms' discriminants guards it. **Head: wp-B65 = 0cfde6a9d on main 3d5464c41** (the
amend of 534f10718; beamlet-vm 108/108, docs PASS, fmt clean after it). Still to do, by the
successor: when B60's docs amend lands, rebase onto wp-B60's head and rewrite shell.md's trap
sentence as history in this commit.

Branch wp-B65 on main 3d5464c41, one commit (first 534f10718, amended to 0cfde6a9d):
- `userland/otp/vm/src/term/cmp.rs`: `compare` answers `Equal` for the same term on one heap
  before any deep compare (the check `compare` already had inside its container loop, moved to
  the head so a bitstring — which takes the atomic fast path — gets it too), with the soundness
  written in the comment: Erlang has no NaN, so no term differs from itself; BEAM's `eq` begins
  the same way. `compare_bits` answers `Equal` for two windows of one buffer (`Arc::ptr_eq`) at
  the same offset and length. Nothing else in the term order moves.
- `userland/otp/vm/tests/terms.rs` (an integration test: the VM's unit-test module is `no_std`
  and has no clock): `a_term_equal_to_itself_is_not_scanned` — a 64 MiB binary compared with
  itself 10,000 times (`=:=` and `==`), and two sub-binary terms over one window, under a 5 s
  bound; equal bytes elsewhere still equal, different bytes still differ and order below.
  **With the shortcut: 0.13 s. Negative** (both shortcuts reverted in a scratch copy of cmp.rs,
  restored after): FAILED, "the same term was read: 40.56 s".
- `docs/userland/beamlet.md` "Limits inside one VM": the rule as a bullet (a term is equal to
  itself without being read; sound for every Erlang term, no NaN; why it matters at the prompt),
  the test in the status list (34 → 35).
- `docs/userland/shell.md`: B60's trap sentence lands with B60 (train 29); B65 then rewrites it
  as history. Not in this commit, since the sentence is not on main yet — the orchestrator's
  sequencing.

Harness after (the same host beamlet build from wp-B65 with the fix; x86, µs a step):

| variant | 4 KiB | 16 KiB | 64 KiB | 256 KiB | 1 MiB | 4 MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| compiled | 0.159 | 0.159 | 0.161 | 0.162 | | |
| evald | 31.5 | 23.7 | 23.3 | 23.3 | | |
| env | 9.4 | 9.5 | 9.6 | 9.5 | 11.2 | 13.4 (was 146.8) |
| envarg | 3.3 | 3.3 | 3.2 | 3.2 | 6.4 | 6.4 (was 67.8) |
| evald (Elixir) | 32.1 | 26.8 | 27.0 | 27.0 | | |
| grapheme (Elixir) | 30.0 | 27.4 | 28.2 | 28.1 (was 54.7) | | |

Flat in the binary's size; the compiled walk unchanged. (The 1 → 4 MiB residue in `env`, 2 µs,
is the collector's: a 4 MiB off-heap binary lowers nothing but raises `gc_bytes_at`'s base.)

Gates at 534f10718 (q): `cargo test -p beamlet-vm -p beamlet-redoubt --features
beamlet-redoubt/fake` 190 passed / 0 failed; `./test-shell` every stage passed; `tools/difftest`
527/527 passed, 0 failed, 21 skipped by design; docs PASS; `cargo +nightly fmt -p beamlet-vm
--check` clean. The VM crate carries no size ceiling (not in tests/size-budget.toml).

### On the machine: shell-text-profile's prompt walk, before and after

B60's case (wp-B60 94b26298b, on main 3d5464c41) with a probe stage added in scratch copies only
(never committed): a walk written at the prompt, `w = fn _, 0, _ -> :ok; <<_::utf8, r::binary>>,
n, w -> w.(r, n - 1, w) end`, 512 steps over 16 KiB and over the whole 256 KiB `ascii`, with
every binary of the prompt's bindings (`ascii`, `mixed`, `page`, `apage`: 448 KiB) in the fun's
environment, printed as `{:stage, :promptwalk, <bytes>, <µs>}`. "Before" is wp-B60's head;
"after" is the same with 534f10718 cherry-picked. Both widths, each tree's own prebuilt, the
machine quiet (guest µs for 512 steps; ms a step in brackets). Logs
`$REDOUBT_TMP/B65/{before,after}-{rv64,rv32}.log`; scratch trees removed.

| walk over | rv64 before | rv64 after | rv32 before | rv32 after |
| --- | ---: | ---: | ---: | ---: |
| 16 KiB | 5,482,124 (10.7) | 3,791,735 (7.4) | 6,251,326 (12.2) | 4,416,868 (8.6) |
| 256 KiB | 35,306,643 (69.0) | 3,398,259 (6.6) | 36,737,849 (71.8) | 4,116,147 (8.0) |

Before, the step grew with the binary (the rest of it, rebound at each call, compared with
itself): 58 ms more at 256 KiB than at 16 KiB, 0.24 µs a byte on rv64. After, flat: the step is
the evaluator's own constant (7-9 ms here, with the case's four large bindings in the fun's
environment), 10× faster at 256 KiB, and the whole case 26.7 s instead of 32.2 s on rv64 (24.3
instead of 30.7 on rv32) with the probe in it. B60's trap sentence in shell.md ("3.6 ms plus
0.18 µs for every byte") is history once B60 lands; B65 rewrites it then.

## Phase 2, as proposed before it was built

- `vm/src/term/cmp.rs`: `eq_exact` answers `true` when `a` and `b` are the same term (same
  `Term`: pointer and space; one heap) before any deep compare, and `compare_bits` answers
  `Equal` for two windows over the same buffer at the same offset and length (`Arc::ptr_eq`):
  the same binary is equal to itself without a scan, as on the BEAM. `compare` (`==`, sorting)
  gains the same identity shortcut at its top. No semantic change: a term is equal to itself.
- Host test (beamlet-vm): a term equal to itself is not scanned — a 64 MiB binary compared with
  itself 10,000 times under a bound a scan could not meet (640 GB of memcmp); plus the harness
  rerun showing `env`/`envarg` flat.
- Floor on the machine: `shell-text-profile` (B60's case, not merged yet) or a line in it: a
  prompt-written walk's step no longer grows with the binary; or a small new case if B60 does
  not land first. Pages: `docs/userland/shell.md`'s evaluator trap sentence (B60's) becomes
  history; `docs/userland/beamlet.md` "Limits inside one VM" or the term section gains the
  rule (a term equals itself without a scan).
- Tier: the change is the VM's term comparison (`term/cmp.rs`), beside the heap and the term
  copy the orchestrator named as Tier A triggers; I would treat it as Tier A (steward-red-2).

Not changed: erl_eval (OTP's), the shell's evaluator.
