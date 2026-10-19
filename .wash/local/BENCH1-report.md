# BENCH1 report: a case's smp per width

Branch `wp-BENCH1`, worktree `.worktrees/BENCH1`, base `origin/main` dfbf2eeba, head **dbca8d046**
(one commit, clean tree).

## Delivered

- `tools/testbench/src/case.rs`: `Boot::smp` is a new `Smp` enum, serde untagged: `List(Vec<u32>)`
  (the form every case uses today, unchanged in meaning) or `PerWidth(BTreeMap<String, Vec<u32>>)`
  (`smp = { rv64 = [1, 2], rv32 = [1] }`). `Smp::for_width(arch)` gives a width's counts: the one
  list, the width's own, or the default `[1]` for a width the map leaves out (the one behaviour
  the assignment asked for; stated on the page). `Case::check` refuses a map key that is not one
  of the case's `arch` and an empty list in either form, so a misspelt width or a count-less one
  cannot leave a width unbooted in silence. `with_harts` (`--smp N`) replaces a map as it did a
  list.
- `tools/testbench/src/main.rs`: `boot_case` iterates `boot.smp.for_width(target.name)` (the
  runner's expansion); nothing else read the field as a `Vec`.
- `tools/testbench/src/launch.rs`: `./launch --system` sets `Smp::List(vec![ask.smp])`.
- `scripts/jobs.mk`: the `smp` core macro took the digits of the `smp` line, which would read `64`
  from `rv64` and lease 64 cores for a map-form case; it now takes the width's own list from the
  map (the width is passed as a second argument from `$(@D)`), the whole list as before otherwise.
  Checked with `make -n`: the scratch case leases `--cores 2` on rv64 and `--cores 1` on rv32,
  `smp-boot` 4, a default case 1.
- `docs/testbench.md`: the `smp` line of the case-file example points to the new paragraph; one
  paragraph after the boot-field list gives the single example, the rule for a width left out,
  the two refusals, the `jobs.mk` lease and `--smp`; the section's status line names the two new
  host tests.

## Tests

- `host:testbench::smp_is_one_list_or_a_list_per_width`: the parse of both forms and the default,
  check() accepting a map and a map with a width left out, refusing a key outside `arch`, an
  empty per-width list and an empty list, and serde refusing a scalar and a scalar per width.
- `host:testbench::a_boot_runs_its_widths_own_hart_counts`: `for_width` on a list (same on both
  widths), on a map (each width its own), on a map with a width removed (the default), and
  `--smp N` over a map.
- The existing `smp_replaces_a_cases_hart_counts_unless_it_keeps_them` adapted to the type.
- Live proof, both widths: a scratch copy of `boot-stack-reservation` with
  `smp = { rv64 = [1, 2], rv32 = [1] }` placed as an untracked `tests/per-width-smp-scratch.toml`
  (no committed case's content edited; removed before the gates; the copy is kept at
  `.tmp/BENCH1/per-width-smp-scratch.toml` with the two job logs beside it):
  `make -f scripts/jobs.mk set CASES=per-width-smp-scratch` →
  `PASS per-width-smp-scratch [rv32, smp=1]`, `PASS … [rv64, smp=1]`, `PASS … [rv64, smp=2]`,
  rc=0: rv64 booted twice, rv32 once.

## Gates (all through `scripts/q` / `jobs.mk`, exit codes)

- `q run --cores 4 -- cargo test -p testbench`: 231 passed, 0 failed, 1 ignored (rc 0).
- `make -f scripts/jobs.mk set CASES="formatting docs host-tests memory-host-tests"`: docs PASS,
  host-tests PASS (67 s), memory-host-tests PASS (64 s); formatting FAIL on the first run (my
  new test code unformatted) → `cargo +nightly fmt -p testbench` → `set CASES=formatting` PASS
  (rc 0). Everything was rerun on the formatted tree before the commit.
- No kernel, steward or case-content change; rv32 untouched by the change (bench host code).
- Not run: the whole bench (Tier A's full gate is the train's).

## Documentation check

- `docs/testbench.md`: updated (above). The `--smp N` paragraph in "On a shared host" and the
  `.smp<N>` log naming at the top are still true and unchanged.
- `README.md`, `GETTING-STARTED.md`, `docs/README.md`: grep for `smp` finds only
  `./launch --arch rv32 --smp 4` in GETTING-STARTED, the launch flag, unaffected; no change needed.
- Other `docs/*.md`: no page describes the `smp` field's form; no change needed.
- `scripts/jobs.mk` header: the boot class comment now says "the case's largest smp on that width".

## Notes and residuals (no change made)

- `jobs.mk`'s `$(sort)` is lexical, so a list such as `[2, 16]` leases 2 cores (pre-existing,
  no case has such a list today); out of scope here.
- `idle.rs:72` warns `field harts is never read` on `origin/main` too; not mine.
- STEWARD9 adopts the field itself (the three steward-latency cases were not touched, as instructed).
