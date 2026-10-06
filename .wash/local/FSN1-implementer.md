# FSN1: `fsd` is renamed `littlefsd`: a file server is named for the format it serves

Tier A by reach (the crate, the manifest, 47 case files, 26 pages), but mechanical: size S, one
reviewable rename with no behaviour change. Needs nothing; lands before EROFS1, so that the
second file server's package writes every name once.

**The owner's decision (2026-10-06):** "we should consider naming fsd more carefully for
multiple fs servers"; "rename fsd to fsd-littlefs or similar". The convention is on
docs/servers/README.md "Naming": a file server is named for the format it serves. The name is
`littlefsd` (the format's name and `d`, as `erofsd` will be): spelled out, because `lfsd` reads
as "log-structured" to half its readers, and the owner asked for the format to be visible.

Run everything natively on this host under the job pool's rules.

## Context rules (read these first)

- **This is a rename.** No line changes meaning. If a rename would change a sentence's sense
  (a page that says "the file server" where two now exist), fix the sentence minimally and
  list it in the report.
- **Use the tools, not your eyes:** `git grep -l fsd`, `sed`, `git mv`; then read every diff
  hunk before committing. One commit for the crate and code, one for the cases and recipes,
  one for the pages, so each is reviewable alone.
- **Reports under 1900 bytes,** detail in `.wash/local/FSN1-report.md`.

## The rename

1. **The crate:** `servers/fsd` → `servers/littlefsd`; the package `redoubt-fsd` →
   `redoubt-littlefsd`; the binary; every `use`; `Fsd` as a type name may stay (it is the
   server's struct, not the format: say whether you kept it); the host-tests case
   `fsd-host-tests` → `littlefsd-host-tests`.
2. **Endpoint and server names:** `fsd:data`, `fsd:system`, `fsd:<volume>` → `littlefsd:...`
   in `image/manifest.json`, `tests/data/**`, every case file's expectations and inputs, and
   the programs' `endpoint=` arguments; the handle names programs look up (`fsd:system` in
   beamlet's manifest entry and its arguments).
3. **Case names:** `fsd-*` cases → `littlefsd-*` (`fsd-one-volume`, `fsd-reboot`,
   `fsd-confined-labelled`, `fsd-large-directory`, …); every page's status line that lists
   them; `.wash/local/jobs.mk`'s class lists if they name them (tell the orchestrator: the
   scheduler is theirs).
4. **The pages:** `docs/servers/fsd.md` → `docs/servers/littlefsd.md` with its title; every
   link (`fsd.md#…`) across the book; SUMMARY.md; SECURITY.md's rows; the server graph and
   tables on servers/README.md; "The file server" where it means this one becomes "the
   littlefs file server" only where two could be meant.
5. **What does not change:** `libs/littlefs` (the format's library keeps its name); the
   `Range` and `Blocks` names; the wire tables (`fsd.md` in `libs/wire/tables` is the 9P file
   protocol table included by the page: rename the file with the page and its include).
6. **The plan's `fsd` step** keeps its id (plan ids are not the book); say so in the report.

## The cases

Every existing case, unchanged in substance, under its new name where renamed. The gate is
the whole bench on both widths: a rename that passes it changed nothing.

## Page lines

Only renames and the minimal sense fixes of point 4, each listed in the report with its old
and new sentence. README.md "Naming" is already written (the Architect's); do not restate it.

## Owned paths

Everything the rename touches, by `git grep -l fsd` at the start, listed in the report: the
crate, `image/**`, `tests/**`, `tools/testbench` (case-kind code that names `fsd`), the pages.
**Not yours:** any behaviour; `libs/littlefs`; the job scheduler.

## Gates

The whole bench on both widths; `cargo fmt --check`; doccheck; the size budget unchanged (a
renamed crate is the same size: say so); the unsafe ratchet unchanged.

## Not here

`erofsd` (EROFS1), any change to what the server does.

## Checkpoint

None: three commits, one report.
