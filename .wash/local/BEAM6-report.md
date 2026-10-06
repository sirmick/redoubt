# BEAM6 report: done (steps 1–3), branch wp-BEAM6

Head 78bd4e6d4, rebased on main a9f54ffcb (FSN1: fsd -> littlefsd taken in). Four commits:

1. 08b6f311e `rt: a program can read its own heap's pages, held now and at its peak` —
   `Heap::held()`, `redoubt_rt::heap_pages()` (cfg target_os=none), size-budget libs/rt 3516 -> 3519
   with its `Size budget:` line. libs/rt joined the owned paths for these lines only (orchestrator).
2. 0dc634200 `beamlet: the VM says what it holds when it first waits for input, under report_memory`
   — memory.rs `footprint`/`held`/`HeapPages`, vm.rs `Config::report_memory` (report at the first idle
   with a console reader; every waiting process collected first), getters in term/mod.rs and atom.rs
   (owned since the ruling), beamlet.rs/fake-redoubt.rs/lib.rs argument `report_memory`, host tests
   `the_footprint_is_reported_at_the_first_wait_for_input`, `held_bytes_follow_the_runtime_heap`
   (limits.erl/.beam fixture `footprint/0`), case `tests/beamlet-footprint.toml`, beamlet.md
   subsection "What the VM holds at its prompt" with the before-levers table.
3. 566b08ac3 `beamlet: loaded code and literal chunks keep no spare room` — levers 1 and 2
   (loader.rs `code.shrink_to_fit`, term/mod.rs `Literals::add` shrinks a chunk), page table updated,
   one sentence per lever, one on eager loading.
4. 78bd4e6d4 `image: the image boots in 512 MiB, beamlet budgeted 20,864 pages` — manifest, eight
   case files' memory_mib 1024 -> 512 (and their comment), README, mkimage comment, budgets.md,
   testbench.md (row, paragraphs, status line names beamlet-footprint), beamlet.md residual on
   256 MiB, servers/init/tests/manifest.rs (its fit test sums the image's budgets: 24_576 -> 20_864;
   **outside my owned paths**, a mechanical consequence of the manifest; flagged for review).

## Numbers (pages; from beamlet-footprint, one run each width)

| Row | rv64 before | rv64 after | rv32 before | rv32 after |
| --- | ---: | ---: | ---: | ---: |
| code: instructions | 2,725 | 1,944 | 1,362 | 993 |
| code: operands | 4,455 | 4,455 | 2,313 | 2,313 |
| literals: per module | 36 | 36 | 36 | 36 |
| literals: shared table | 1,100 | 753 | 1,096 | 749 |
| module tables | 231 | 231 | 202 | 202 |
| atoms | 92 | 92 | 64 | 64 |
| processes (19): heaps / rest | 33 / 100 | 33 / 100 | 33 / 95 | 33 / 95 |
| ETS + binaries | 1 | 1 | 1 | 1 |
| accounted | 8,774 | 7,647 | 5,204 | 4,488 |
| runtime held at prompt, peak | 9,003, 9,003 | 7,876, 7,897 | 5,331, 5,331 | 4,616, 4,622 |
| not accounted (held - accounted) | 229 | 229 | 127 | 128 |
| scan peak after one command | 10,519 | 9,369 | 6,243 | 5,511 |

Accounting line (ruling 3): free-but-held is 229 / 128 pages, under 500: no lever. The checkpoint's
"unaccounted 1,745" was mostly after the prompt: the scan's peak includes the first command's loads.

## Target: 512 MiB met; 256 MiB not

- SystemFit at 512 MiB (probe boot, never committed): today's image needs 34,955 pages; system has
  31,672 free on rv64 and 31,626 on rv32. Servers other than beamlet need 10,378.
- Largest beamlet scan peak across init-boot, userland-boot, userland-read-only, beamlet-footprint,
  one run each per width (8 runs, at 1 GiB and again at 512 MiB, identical): 10,387 rv64
  (userland-read-only), 6,048 rv32.
- Budget 20,864 = heap cap 20,846 (>= 2 x 10,387 = 20,774) + stack 17 + 1, rounded up to 128.
  MEM2's round-up to 1,024 (21,504) does NOT fit: userland-read-only and image-disk add a 256-page
  client, and 21,504 needs 31,628+ > 31,626 on rv32. Window is [20,792, 20,990].
- **Margins are thin:** image alone 383 pages spare on rv32; with a 256-page client 126; heap cap
  over twice the peak: 72 pages (a peak of 10,424+ would fail the scan's cap rule).
- 256 MiB: residual on beamlet.md (built section: C1 refuses **Open:** outside planned sections, so
  it is a "Residual:"): ~2,700 available vs ~5,450 even with compact code.

## Departures from the ruling, for the Architect

- Point 6's own status line on the subsection: doccheck C1 refuses a status under a section that has
  one, so the three tests are in "Limits inside one VM"'s list (17), the subsection has none.
- Point 4's **Open:** line is a "Residual:" (C1, as above).
- Budget rounding to 128, not MEM2's 1,024 (fit, above).

## Gates (all exit 0)

`make -f .wash/local/jobs.mk` (pool): build-rv64, build-rv32, docs, rv64/formatting, size-budget,
unsafe-budget (unchanged count), no-cruft; both widths at 512 MiB: beamlet-footprint, beamlet-boot,
beamlet-console, beamlet-heap-flood, beamlet-budget-flood, userland-read-only, verity-flipped-tree,
verity-wrong-root, userland-boot, init-boot, image-disk, userland-bad-start, ipc-outcomes;
bench-net-peer rv64 (rv32: see the member_update). Host: `jobserver bounded cargo test -p beamlet-vm
-p beamlet-redoubt` (otp workspace, all ok, limits 14), `-p redoubt-rt heap` (2+4), `-p redoubt-init`
(manifest 50 + others ok). Not run: the shell's `mix test` (no bench case runs it; userland/shell
untouched), the whole bench (train's), rt's full host tests (alone-class).

## Summaries checked

docs/userland/beamlet.md (subsection, statuses), docs/testbench.md (memory budget), docs/kernel/
budgets.md (RAM paragraph), image/README.md, mkimage, tests' 1 GiB comments: updated. README.md,
GETTING-STARTED.md, userland/otp/README.md, docs/servers/init.md: grep for 1 GiB / -m 1G / 24,576 /
11,814 finds nothing else; no change needed. .wash/plan.toml's BEAM6 body cites old numbers (not
mine to edit).

## Fold round (Architect conditions, editor and red notes): head ed114b5d1

Six runs per memory case per width (rounds 1-6: 1 at 1 GiB, 5 at 512 MiB), heap beamlet peak,
identical in every run:

| Case | rv64 | rv32 |
| --- | ---: | ---: |
| init-boot | 1 | 1 |
| userland-boot | 9,965 | 5,837 |
| userland-read-only | 10,387 | 6,048 |
| beamlet-footprint | 9,369 | 5,511 |

Cap unchanged: 20,846 (72 over 2x 10,387); budget 20,864. Folded: memory.rs total counts the
collected heaps (red P2-2; accounted 7,660/4,502, unaccounted 216/114 after, 215/114 before:
confirmed by a run, 7,660 and 4,502 exact); rows-rounded note; code fraction "about four fifths on
rv64 and seven tenths on rv32" (P2-3); transient wording and blank line; residual reworded; budgets.md
"plus its stack", 126 explained (the client's 257 pages with its budget page), tripwire sentence
with the window [20,792, 20,990]; testbench.md paragraph with rounding and margins. Editor item 3
unchanged per the Architect. Gates after fold: docs, formatting, beamlet-footprint both widths,
limits host tests (14) rc=0.
