# B78 — the book's search index gets a stated headroom

**Head.** The index was 60 % too large because of the indexer, not the book: elasticlunr-rs 3.0.2
(pinned by mdbook 0.5.4's lockfile, which `--locked` honours) indexes each field with the tokens
of the fields before it, so `breadcrumbs` carried the whole body vocabulary; 3.1.0 fixed it.
`scripts/setup.sh` now installs `mdbook@0.5.4` resolved fresh (comment names the bug), the host
was reinstalled (elasticlunr-rs 3.1.0 in the binary), and today's book indexes to
**6,093,217 bytes** (was 9,928,026) at heading-split-level 3, every rule heading its own hit.
Ceiling **7,000,000** bytes (30 % under mdbook's 10,000,000 WARN; 0.9 MB ≈ 1.5 recent weeks of
prose above today at 3.2× index per byte of text) in `the_book_builds`, failing with the size and the three largest pages; the
same test reads the elasticlunr-rs version from the mdbook binary and refuses < 3.1.0.
`docs/README.md` states the ceiling and the headroom rule. No page split or removed; book.toml
gains only a comment (no knob was worth setting: see the table).

**Gates (reinstalled host, 2026-10-16).**
- `scripts/q run -- cargo testbench docs` → PASS docs 2.7 s, exit 0 (a pre-existing `harts`
  dead_code warning from another crate scrolls by in the build output; not this package's).
- `scripts/q run -- cargo test -p redoubt-doccheck` → rules 23 passed (incl. the two new:
  `index_over_its_ceiling_names_the_largest_pages`, `elasticlunr_version_is_read_from_the_binary`),
  docs 2 passed, exit 0.
- `mdbook build docs`: no WARN/ERROR (only the mermaid version notice).
- Hand search on the host's index: R41 → `servers/steward.html#r41-narrowing-by-revocation-scope`
  (title hit); budget_pages → `kernel/budgets.html#the-tree-from-the-boot-manifest`, `#r6-charging`;
  walfsd → `servers/walfsd.html#walfsd` (title hit). 1,078 sections.
- `cargo +nightly fmt -p redoubt-doccheck -- --check` clean; `git diff --check` clean;
  `cargo run -q -p redoubt-doccheck` no findings; `bash -n scripts/setup.sh` ok.
- Negative: fixture `tools/doccheck/tests/fixtures/index` (1,491-byte index, pages 2,807/1,687/1,129/120
  bytes) at ceiling 1,000 fails naming `1491 bytes`, `ceiling of 1000`, big/mid/small in order,
  not tiny; at 1,491 returns Ok.

**Files.** docs/book.toml (comment), docs/README.md (+ "The search index"), docs/testbench.md
(status 8→9, two sentences), GETTING-STARTED.md (install note), scripts/setup.sh (mdbook without
--locked), tools/doccheck/src/lib.rs (`search_index_within`, `elasticlunr_version`),
tools/doccheck/tests/docs.rs (ceiling + indexer guard), tools/doccheck/tests/rules.rs (2 tests),
tools/doccheck/tests/fixtures/index/.

**Summaries checked.** README.md: no mdbook/search claim, no change. GETTING-STARTED.md:47-51
named `--locked` for all four tools: updated. CONTRIBUTING.md:35-38 (run the checker, render the
book): still true, no change. docs/testbench.md "The docs checker" and "in the bench": updated as
above. docs/README.md "Rendering": the new subsection. docs/milestones: no claim about the book's
tooling, no change.

**Risks.** The ceiling buys ~0.9 MB ≈ 1.5 weeks at the recent rate (index ≈ 3.2× prose with the fixed indexer: a week's ~190 KB of prose is ~0.6 MB of index); the rule
in README is the durable lever and the next failure names the pages. The version guard reads a
registry path from the binary: a stripped/remapped build would fail with "no elasticlunr-rs
version in the binary" (the ceiling remains the honest guard). setup.sh's skip check
(`cargo install --list` shows `mdbook v0.5.4`) does not see which indexer an old binary links;
the comment and GETTING-STARTED name `--force`, and the test says so when it fails.

## Measurements

Host: mdbook v0.5.4 at `~/.cargo/bin/mdbook` (installed 2026-10-04, built against
elasticlunr-rs 3.0.2). mdbook's WARN (`mdbook-html/src/html_handlebars/search.rs:57`) fires when
the serialized JSON exceeds 10,000,000 bytes; the file on disk, `target/book/searchindex-<hash>.js`,
is that JSON in a JS wrapper with `\` and `'` escaped: 33,671 bytes more today. The ceiling test
measures the file, so it is conservative by that margin.

Book at main e42f63193: 1,929,332 bytes of Markdown under `docs/`; largest pages testbench.md
136,294, kernel/scheduling.md 125,370, userland/beamlet.md 105,358, userland/shell.md 93,464,
servers/steward.md 90,235.

| `[output.html.search]` | file bytes | JSON bytes | under WARN | sections | R41 | budget_pages | walfsd |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| enable only (heading-split-level 3) | 9,928,026 | 9,894,355 | 1.1 % | 1,077 | title hit, `steward#r41-narrowing-by-revocation-scope` | `budgets#the-tree-from-the-boot-manifest`, `#r6-charging` | title hit, `walfsd#walfsd` |
| expand = false | 9,928,027 | 9,894,356 | 1.1 % | 1,077 | same | same | same |
| teaser-word-count = 15 | 9,928,026 | 9,894,355 | 1.1 % | 1,077 | same | same | same |
| limit-results = 20 | 9,928,026 | 9,894,355 | 1.1 % | 1,077 | same | same | same |
| heading-split-level = 2 | 8,786,879 | 8,754,273 | 12.5 % | 512 | body only: `steward#interface`, `#authority`, `#security-properties` | `budgets#interface`, `#security-properties` | title hit, `walfsd#walfsd` |
| heading-split-level = 1 | 7,611,447 | 7,579,623 | 24.2 % | 80 | `steward#the-steward` (page top) | `budgets#budgets` (page top) | `walfsd#walfsd` |
| expand=false + level 2 | 8,786,880 | 8,754,274 | 12.5 % | 512 | as level 2 | as level 2 | as level 2 |
| expand=false, level 1, teaser 15, limit 20 | 7,611,448 | 7,579,624 | 24.2 % | 80 | as level 1 | as level 1 | as level 1 |

`expand`, `teaser-word-count` and `limit-results` are query-time options serialized into the
file's header (`search_options`, `results_options`); they do not touch the index (±1 byte). Only
`heading-split-level` changes what is indexed, and it trades section anchors for bytes: at level 2
a `###` rule such as R41 is no longer its own document and a hit lands on the enclosing `##`
section; at level 1 every hit lands on the page top.

Search check method: the file's JSON parsed, the elasticlunr trie walked for the stemmed term
(`r41`, `budget_pag`, `walfsd`) in the `title` and `body` fields, the document refs mapped through
`doc_urls`. Script: `.tmp/b78/measure.py` (scratch, not committed).

### Composition of today's index (level 3)

| part | bytes |
| --- | ---: |
| documentStore (the sections' text, for teasers) | 1,972,114 |
| inverted index `title` | 80,202 |
| inverted index `body` | 3,909,004 (10,183 terms, 93,375 postings) |
| inverted index `breadcrumbs` | 3,947,708 (10,184 terms, 94,718 postings) |

The `breadcrumbs` field stores 43,959 characters of text in total, yet its inverted index holds
the body's whole vocabulary: elasticlunr-rs 3.0.2's `Index::add_doc` declares `token_freq` once
per document and never clears it between fields, so field *i* is indexed with the tokens of
fields 0..=*i* (`elasticlunr-rs-3.0.2/src/lib.rs:314-347`). 40 % of the file is that
duplication. elasticlunr-rs 3.1.0 (2026-07-25) fixes it (CHANGELOG: "Fixed tokens from earlier
fields leaking into later fields' indices", mattico/elasticlunr-rs#60); mdbook-html 0.5.4
requires `elasticlunr-rs = "3.0.2"` (caret), so a `cargo install mdbook` resolved today links
3.1.0. The host's binary predates that only by its install date.

### Growth

Markdown under `docs/` at main's first-parent history: 45,020 bytes until 2026-09-15 (the book
was elsewhere), 1,233,352 on 2026-10-01, 1,738,933 on 2026-10-08, 1,929,332 on 2026-10-16. The
old indexer's output was ≈ 5.1× the Markdown; the fixed indexer's is ≈ 3.2× (6,093,217 /
1,929,332), so the recent week's 190 KB of prose is ≈ 0.6 MB of index, and the ceiling's 0.9 MB
above today is ≈ 1.5 such weeks.

### The fixed indexer, measured

`cargo install mdbook --version 0.5.4 --root .tmp/b78/mdbook` (no `--locked`, so the
dependencies resolve today: elasticlunr-rs 3.1.0) and `mdbook build docs` with that binary,
book.toml unchanged (heading-split-level 3):

| | file bytes | JSON bytes | under WARN | sections | R41 | budget_pages | walfsd |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| fresh mdbook 0.5.4, enable only | 6,087,464 | 6,055,419 | 39.4 % | 1,077 | title hit, `#r41-narrowing-by-revocation-scope` | `#the-tree-from-the-boot-manifest`, `#r6-charging` | title hit, `#walfsd` |

documentStore 1,972,114 (unchanged), inverted index 4,094,716 (was 7,936,947). No mdbook warning.

### Why no reproducible pin today

A `--locked` install takes the published crate's own lockfile, and mdbook 0.5.4 is the newest
release (`cargo search mdbook`: 0.5.4), with elasticlunr-rs 3.0.2 in that lockfile; a registry
install cannot be given a patched lockfile, and `cargo install --git` of a fork pinned to a
commit would be a fork of mdbook for one dependency bump: disproportionate for a dev tool. So
setup.sh resolves mdbook fresh and the checker holds the result (the elasticlunr-rs version read
from the binary, and the index size at its ceiling). The comment in setup.sh names the trigger
for restoring `--locked`: an mdbook release whose own lockfile is at elasticlunr-rs >= 3.1.0.
Host figures: the scratch install under `.tmp/b78/mdbook` indexed 6,087,464 bytes; the host's
reinstalled `~/.cargo/bin/mdbook` 6,093,217 (one more section: this package's README
subsection); the test comment quotes the host's.
