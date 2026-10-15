# B72: beamlet is told its budget by the startup block

Branch wp-B72, worktree .worktrees/B72. Base main 985ec0047; head 6e33ab324 (one commit; round 1
was 42d9c1386).

## Round 2 (steward-red-2's three notes, folded and amended)

- P2-1: no `budget_pages` map key. `Kind::Budget { pages }` in beamlet_redoubt::system records
  the pages at `budget_create` (spec.pages) and, for the VM's own named budget, what its own block
  told it (0 for none); `budget()` returns `(Handle, pages)`; `Resolved.budget_pages` carries it to
  jobs.rs, which writes it into the child's block. Launch (platform.rs), the bif's field list,
  process.ex and beamlet.md's launch map lose the key. Test: a_launch_takes_what_it_is_given_and_its_end_is_an_event
  now launches into a carved budget of 64 pages and reads Some(64) back from the child's block (and
  None for a child in the VM's own budget, whose fixture block names none); the fake kernel gained
  `launched_under(owner, parent)` (libs/rt/fake) to find children in budgets whose handles the test
  never sees. beamlet-session.rs already filled the field itself as a launcher; unchanged.
- P3-1: check.rs `blocks()` is now a loop over `pub fn block(m, i, machine, bundle_key) -> Vec<u8>`;
  servers/init/tests/manifest.rs `a_servers_block_carries_its_budgets_pages` parses every image
  server's block and asserts budget_pages == budget.pages (and heap_pages == the manifest's).
- P3-2: `redoubt_wire::STARTUP_VERSION = 3` (libs/wire/src/lib.rs, hand-written beside the
  tables); rt's and the stub's `VERSION` are that constant. The stub's
  `read_image_reads_the_field_a_real_builder_writes` still parses a block the runtime's builder
  wrote, so a drift between them fails it.
- init.md's startup-block status list names the two new host tests (tested (17)).
- Size budget: libs/wire 3908 (+1 for the constant), servers/init 2559 (+9 for block()).

Round-2 gates (head 6e33ab324; the last amend over 4c43b970a changed only init.md's count and two
ceilings, no code): host tests redoubt-rt, redoubt-client, stub, redoubt-init,
redoubt-steward-server, testbench rc 0 (host2.log); beamlet-redoubt + beamlet-vm rc 0 (otp3.log);
build-rv64 and build-rv32 rc 0; prebuilt rc 0; init-boot, launch-system, beamlet-footprint,
steward-home-quota PASS at rv64 and rv32 (round2.log); formatting PASS; size-budget PASS;
docs PASS.

## What changed

- The startup block (libs/wire/tables/startup.md; codec regenerated with
  `cargo run -p redoubt-wire-gen`: libs/wire/src/proto/startup.rs, libs/wire/elixir/{proto,client}/startup.ex)
  gains `budget_pages: u64` between `heap_pages` and `tag`; VERSION 2 -> 3 in libs/rt/src/startup.rs
  and in stub/src/lib.rs (the stub pins the version too; its host tests caught it). `u64`, not `u32`:
  the manifest's `budget.pages` and the kernel's budget records are u64, and a saturated value would
  be a lie; heap_pages stays u32 as the manifest bounds it.
- `Startup::budget_pages() -> Option<u64>` (0 = none), `StartupBuilder::budget_pages`,
  `redoubt_client::Launch::budget_pages`.
- Launchers fill it: init at the check (check.rs `blocks`) and at launch (init.rs) with
  `s.budget.pages`; the steward's `launch` with `vm_pages` (session less RELAY_PAGES, as before) and
  `launch_relay` with RELAY_PAGES (the relay's share of the same budget); beamlet's `launch` native
  takes a new optional map key `budget_pages` (platform.rs `Launch`, bif/system.rs, jobs.rs,
  process.ex doc); tests/beamlet-programs beamlet-session.rs passes SESSION_PAGES through the block.
- beamlet reads `startup.budget_pages()`; none -> BAD_ARGS (4), the old missing-argument code.
  `BUDGET_PAGES` and `budget_pages()` are gone from beamlet_redoubt (and their two host tests).
- The argument is gone from every manifest: tests/data/beamlet/{boot,console,flood,budget-flood,files}.json,
  tests/data/beamlet-footprint/manifest.json, tests/beamlet-footprint.toml, servers/init/tests/manifest.rs,
  userland/otp/redoubt/tests/system.rs. image/manifest.json never named it (the steward starts the VM).
- Bench host test `every_beamlet_is_told_its_own_budget` -> `no_manifest_names_budget_pages`
  (tools/testbench/src/case.rs): every server of every manifest, still requiring >= 4 beamlet servers found.
- Fuzz target libs/rt/fuzz asserts `budget_pages() != Some(0)`.
- tests/size-budget.toml raised, with `Size budget:` lines in the commit: libs/client 1109->1118,
  libs/rt 3927->3943, libs/wire 3905->3907, servers/init 2546->2550 (steward fell by 1).

## Consumer sweep (every crate linking redoubt_rt)

`grep -l redoubt_rt` list (29 + beamlet-vm): redoubt-rt, redoubt-client, redoubt-fileserver,
redoubt-blkd, -bootfsd, -consoled, -consrelay, -erofsd, -init, -ipd, -keyd, -littlefsd, -netd,
-piped, -sshd, redoubt-steward-server, -verityd, -walfsd, stub, redoubt-beamlet-programs,
-erofsd-programs, -init-programs, -littlefsd-programs, redoubt-net-tests, test-programs,
redoubt-walfsd-programs, redoubt-sshd-host, testbench; beamlet-redoubt, beamlet-vm.

- Host build + tests, root workspace: `q run -- cargo test -p <all 28>`: 108 suites ok, rc 0 after
  the stub fix (first run: stub 3 read_image tests failed on VERSION 2; fixed, `cargo test -p stub`
  23 passed). Log .tmp/B72/host.log, stub.log.
- Host tests userland/otp: `cargo test -p beamlet-redoubt -p beamlet-vm`: 16 suites ok, rc 0 (otp.log).
- Cross builds both widths: `make -f scripts/jobs.mk build-rv64 build-rv32` rc 0 (cross.log; run before
  the one-line stub VERSION fix; prebuilt rebuilt everything after it, rc 0).
- `cargo +nightly check` of libs/rt/fuzz: rc 0.

## Gates

- docs: PASS (make docs). formatting: PASS. size-budget: PASS (after the commit).
- Cases (make -j4 set CASES="init-boot launch-system beamlet-footprint steward-home-quota"
  WIDTHS=rv64,rv32): all 8 PASS, rc 0 (log .tmp/B72/cases.log): init-boot rv64 6.9s / rv32 6.7s;
  launch-system rv64 5.3s / rv32 4.3s; beamlet-footprint rv64 5.3s / rv32 6.3s; steward-home-quota
  rv64 195.4s / rv32 198.7s.

## Summaries checked

- docs/servers/init.md: startup block table (version 3, new row saying who writes what),
  "Using it", status list (renamed rt test). Manifest table unchanged: `budget` was already there.
- docs/userland/beamlet.md: limits section says the budget comes from the block and names the new
  bench host test; restart-rule sentence; `launch` map gains `budget_pages`.
- docs/kernel/budgets.md:231 and docs/servers/steward.md:579: "its `budget_pages`" -> "its startup
  block's `budget_pages`".
- docs/userland/native.md: mentions heap_pages only in the heap row; the launch section does not
  list the map keys; no change needed.
- docs/todo/beamlet-budget-from-startup.md deleted, SUMMARY.md line removed; docs/todo/README.md did
  not list it. README.md, GETTING-STARTED.md, docs/README.md, milestone pages: grep for
  budget_pages/the todo found nothing; no change needed.
- libs/wire/tables/startup.md is included by init.md; libs/rt/src/startup.rs module doc table updated.

## Notes and risks

- Stable `cargo fmt --all` was run by mistake once and reformatted the tree; every file not in the
  package was reverted with `git checkout --` and nightly fmt re-run; `git status` showed exactly the
  package's files afterwards and the formatting case passes.
- Files committed were reviewed via the full diff and the regions edited, not every large file end
  to end (the launch's context rules forbid whole-file reads of large files): check.rs, init.rs,
  steward.rs, case.rs, system.rs, the four doc pages.
- init's host tests exercise the field only through `blocks()` (every manifest test builds blocks
  with the budget's pages); no test asserts the value written. redoubt-client's launch test and
  rt's round trip assert it.
- The relay is told RELAY_PAGES, its share, not the whole session budget: the block's doc row says
  "the pages of the budget the launcher made for the child, or the child's share of one it divides".
