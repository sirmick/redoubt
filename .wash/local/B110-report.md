# B110 report: testbench.md's case counts held to the case files

Branch wp-B110, base 7a15bc867, head 69ccdde58 (one commit; 4bdd5e0df before the editor's fold). Host-only; no boots.

## Delivered

- `tools/doccheck/src/lib.rs`: rule C14 (`case_counts`). `STATED` lists 8 fixed phrases on
  docs/testbench.md, each with `{}` where a number stands, and the class of case files each number
  counts. Paragraphs outside fences are joined across line breaks before matching; numbers are
  digits (commas allowed) or words zero..twenty ("six"). Classes from `tests/*.toml` (the files
  the bench reads, same top-level `*.toml` walk), parsed with `toml`: boot; under `icount`; at
  `shift=3,sleep=off`; at `shift=3`; on the host's clock (boot, no `icount`); host-clock with a
  disk (`disk` or `userland`), with host sockets (`net.forward/poke/peer/dial` set, a list
  non-empty), with `[[input]]`, with none of these; `ssh-loopback`. A mismatch is a finding at the
  number's line; a phrase missing from the page is a finding at line 1, so a reworded count
  cannot silently escape (reword the checker too). A case file that does not parse is a finding.
  C4's excuse for checker rule names on testbench.md now runs to C14; module doc says C1..C14.
- `tools/doccheck/tests/fixtures/c14/` (page + 3 case files) and `good/` (paragraph with every
  phrase true, one broken inside a code span; 7 case files). `rules.rs`: `c14_case_counts` in
  `fires!`, and `c14_reports_each_failure` asserting lines {1,3,4,8}: drifted boot count, a count
  past a line break, a count in words, a phrase not stated; true counts beside them and a fenced
  count pass.
- `docs/testbench.md`: recount (352→355 boot, 165→168 host clock twice, 138→141 disk, 70→72
  sockets; 187/180/six/28/15/14 were right); "What it checks" lists C14 and the new test (9→10).

Total case files is 425; the page states no total, so no phrase checks it (the ruling's "if stated").
Not derivable and left by hand: "13 of the 15" (13 = the listed host-time runs) and "13 of the 14"
loopback cases asserting no time; the 15 and 14 are checked.

## Red on the stale text

Before the recount, `cargo test -p redoubt-doccheck --test docs docs_follow_the_rules` exit 101:

```
docs/testbench.md:293: C14: states 352 boot cases; the case files hold 355
docs/testbench.md:295: C14: states 165 boot cases on the host's clock; the case files hold 168
docs/testbench.md:296: C14: states 138 host-clock cases with a disk; the case files hold 141
docs/testbench.md:297: C14: states 70 host-clock cases with host sockets; the case files hold 72
docs/testbench.md:325: C14: states 165 boot cases on the host's clock; the case files hold 168
```

## Gates (on 4bdd5e0df unless noted)

- `q run --cores 4 -- cargo test -q -p redoubt-doccheck --no-fail-fast`: exit 0 (25 rules + 2 docs).
- `make -f scripts/jobs.mk set CASES="docs formatting"`: exit 0; PASS docs 2.1 s, PASS formatting 22.9 s.
- `cargo clippy -p redoubt-doccheck --all-targets -- -D warnings`: one finding, pre-existing on
  main (`clippy::some_filter`, lib.rs:796 `rule_heading`, not this diff); with that lint allowed,
  exit 0. Not fixed: outside the package.
- Not run: boots, whole bench (host-only package).

## Summaries checked

- docs/testbench.md "The docs checker / What it checks": updated (C14, test list).
- docs/README.md "Rendering" (links the checker, no rule list): no change.
- CONTRIBUTING.md (runs doccheck, no rule list), README.md, GETTING-STARTED.md: no case counts
  or rule list; no change. Grep of docs/ and root pages for other stated case counts: only
  testbench.md's ("train 40: 30 cases" is a history figure, not a count of case files).
- Milestone progress: tooling only, no change.

## Note for B109

B109's two twins will move the counts (boot 355→357/358 and the host-clock reasons); C14 will
name each line to update in the docs case.

## Editor fold (OK with notes, three P3s), head 69ccdde58

1. `case_files` now reads `tests/` with `read_dir`, top-level files only. The old `walk(.., &|_| true)`
   already skipped subdirectories (a `true` skip never recursed), but that wasn't obvious. A boot-kind
   `tests/data/nested.toml` in the c14 fixture pins it: if nested files were counted, line 3's
   "of the 3" would pass and the test would fail.
2. `c14_reports_each_failure` asserts the exact (line, message) list: 1 missing loopback phrase,
   3 boot 3/2, 4 shift=3 2/0, 8 host clock 5/1.
3. 'What it checks' now says the two numerators (13 of 15, 13 of 14) stay by hand: they depend on
   a case's purpose, which no field of its file states.

Rerun: cargo test -p redoubt-doccheck --no-fail-fast exit 0; jobs.mk set CASES="docs formatting"
exit 0 (PASS docs 1.8 s, formatting 22.9 s); clippy (some_filter allowed) exit 0.
