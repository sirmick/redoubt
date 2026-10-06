# FSN1 report: `fsd` -> `littlefsd`

Branch wp-FSN1, worktree .worktrees/FSN1, on main 9a3dfcc89. Three commits, signed off:

- 354cc5161 littlefsd: the littlefs file server is named for the format it serves (crate, code,
  wire table + regenerated proto, client module, test programs, image/**, mkimage, budgets).
  Carries `Size budget: servers/fsd: ...` and `Unsafe budget: fsd (...): ...` lines.
- 67fa0352d tests: the littlefsd cases and recipes say its new name (tests/*.toml, tests/data/**)
- 1709f6943 docs: the book calls the littlefs file server littlefsd (docs/**)

156 files against main 9a3dfcc89, +1175/-1158. Every changed line was checked mechanically: undoing
littlefsd->fsd / Littlefsd->Fsd on each + line gives back its - line, except the hunks below,
read by hand (rustfmt reflows of lengthened lines, Cargo.lock reordering, and these edits).

## What was renamed

- `servers/fsd` -> `servers/littlefsd`, `redoubt-fsd` -> `redoubt-littlefsd`, bin `fsd` -> `littlefsd`,
  `tests/fsd.rs` -> `tests/littlefsd.rs`, `src/bin/fsd.rs` -> `src/bin/littlefsd.rs`.
- Struct `Fsd` -> `Littlefsd`, `Fsds` -> `Littlefsds` (I renamed it, not kept it); test fns ending
  `_fsd` / `fsds_` follow.
- `tests/fsd-programs` -> `tests/littlefsd-programs` (`redoubt-littlefsd-programs`), `fsd-client` -> `littlefsd-client`.
- Wire: `libs/wire/tables/fsd.md` -> `littlefsd.md`, marker `wire: littlefsd ninep`; regenerated
  `proto/littlefsd.rs`, `elixir/proto/littlefsd.ex` (`Redoubt.Wire.Proto.Littlefsd`); the
  generator's output equals the sed result. No byte on the wire depends on the name.
- Client: `redoubt_client::fsd` -> `redoubt_client::littlefsd` (now imports `Protocol` directly, so
  rustfmt does not reflow it past libs/client's size ceiling).
- Endpoints/handles: `fsd:data`, `fsd:system`, `fsd:a`, `fsd:b`, `fsd:alice-secrets`, ... ->
  `littlefsd:*` in image/manifest.json, tests/data/**, cases, beamlet.rs's lookup, endpoint= args.
- Cases: all 12 `fsd-*` -> `littlefsd-*` (incl. `littlefsd-host-tests`, `littlefsd-build`).
- Pages: docs/servers/fsd.md -> littlefsd.md (title), every link, SUMMARY.md, SECURITY.md rows,
  servers/README.md graph/tables, files.md figure (node id FSD -> LFSD).
- Budgets: size-budget key `servers/fsd` -> `servers/littlefsd` (same ceiling; the crate is the same size);
  unsafe-budget entry renamed, same limits.
- Grammar: "an `fsd`" became "a `littlefsd`" (14 places, two in littlefsd-client.rs; not sense changes).
- jobs.mk class lists name no fsd case: nothing for the scheduler.
- .wash/plan.toml's `fsd` step and FSD1..FSD4 ids are unchanged (plan ids are not the book).

## Sense-fixed sentences

1. docs/servers/littlefsd.md:3
   old: "`fsd` is the file server: one instance per volume, ..."
   new: "`littlefsd` is the littlefs file server: one instance per volume, ..."
   No other. "The file server" elsewhere (files.md, serving.md, wire.md, m1-separation.md, ...)
   still means the one file server of a session's files; none could mean bootfsd, and erofsd does
   not exist yet. unsafe-budget's label "littlefsd (the file server; ...)" is left as it was.

## `git grep -n fsd` residue (excluding bootfsd/littlefsd)

- bios/firmware/runtime/src/trap/decode.rs:84: the RISC-V `fsd` instruction (flw/fld/fsw/fsd).
- libs/wire/tests/vectors.rs:189, :374: `"fsd:data"` is an arbitrary sample string pinned in hex
  by the hand-written libs/wire/vectors/example.txt; renaming it fails `example_vectors` and
  would mean re-deriving conformance vectors. It names no server.
- libs/wire/fuzz/seeds/json/00, typed/05 (binary): fuzz corpus inputs, arbitrary bytes.
- vendor/** (`fedfsDescr`, a base64 string): third-party.
- .wash/plan.toml, .wash/qa/**: plan ids and history.
- servers/init/fuzz/seeds/check: only `bootfsd` (and a mutated `btfsd`).

2. docs/servers/README.md "Naming" (from main 9a3dfcc89), per the orchestrator's instruction:
   old: "**Open:** `fsd`, which serves littlefs, still carries its role's name."
   new: "**Open:** none."
   The paragraph above it is renamed only (its link `fsd.md#r47-...` -> `littlefsd.md#r47-...`).
   Its status line still says "planned · M1"; I left it: littlefsd now follows the rule, and
   erofsd, which the paragraph also names, is still planned.
3. Renamed only, from main 9a3dfcc89: docs/servers/erofsd.md (9 lines: links to fsd.md and
   "as `fsd` is/serves" -> littlefsd), docs/SUMMARY.md's entry, servers/README.md's graph,
   tables and holdings rows, and fsd.md's new littlefs paragraph (it moved with the page).

## Rebase

Rebased with --signoff onto main 9a3dfcc89 (from bd6f768f6). Conflicts were only in the docs
commit (SUMMARY.md, servers/README.md): I took main's text and re-ran the same substitution.

## Gates (short gate, via the pool; exit codes)

- build-rv64 rc=0, build-rv32 rc=0 (after the final commit).
- docs rc=0, formatting rc=0, no-cruft rc=0, size-budget rc=0, unsafe-budget rc=0 (after the final commit).
- Host tests (`jobserver bounded cargo test -q -p X`): redoubt-littlefsd 0, redoubt-client 0,
  littlefs 0, redoubt-rt 0, redoubt-wire 0 (after the vectors fix; it was 101 before), redoubt-wire-gen 0,
  redoubt-blkd 0, redoubt-init 0, redoubt-ipd 0, redoubt-verityd 0, testbench 0;
  model-host-tests (bounded case, rv64) PASS rc=0. A first loop over these was killed at its
  1 h background limit inside redoubt-model; rerun as above.
  libs/littlefs/diff (own workspace, C reference) `cargo test`: 0.
  Bench host cases: littlefsd-host-tests, littlefs-host-tests, beamlet-lookup-host PASS.
- Boot cases, both widths, PASS: littlefsd-{boot,confined-labelled,corrupt-volume,label-check,
  large-directory,one-volume,quota,reboot,restart}, littlefsd-build, beamlet-{boot,budget-flood,
  console,heap-flood}, image-disk, init-refuses-stack, userland-boot, userland-bad-start,
  verity-flipped-tree, verity-wrong-root, bench-net-peer, ipc-outcomes.
- Failing, and failing on main bd6f768f6 too on this host today (run in a temporary detached
  worktree of main, since removed):
  - aio-many-reads-two: flaky on both (branch passes and fails on the same tree; main 2 PASS, 1 FAIL
    "receive: Timeout"). aio-many-reads passes both widths on rerun.
  - userland-read-only: times out at the shell's `1.2.3` prompt line, both widths, on main too
    (main rv64 and rv32 FAIL, same expect).
  - init-boot: "beamlet: no heap record found". Main rv64 FAIL the same way; branch rv64 PASS on
    rerun, rv32 FAIL. The memory dump comes right after "the boot is done" and races beamlet's
    first allocation.
  I tried renaming the binary to other names (fsd, littlefsx, abc, abcdefgh): aio results moved
  with code layout, not with name length or the endpoint names. I read that as the cases' existing
  timing or layout sensitivity, not something the rename did. Worth a look, but outside FSN1.
- Not run: the whole bench and alone-class cases (client-host-tests, r4/rt-host-tests; the
  train's). redoubt-client's own `cargo test` ran instead: 0.

## Summaries checked

README.md (names bootfsd only: no change), GETTING-STARTED.md (no fsd: no change),
docs/plan/m1-separation.md and m5-persist.md (renamed), docs/servers/README.md (renamed),
image/README.md (renamed), crate READMEs: servers/littlefsd and libs/client have none.
