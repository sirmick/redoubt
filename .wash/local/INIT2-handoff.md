# INIT2 handoff (init2-implementer → init2-implementer-2)

## State
- Branch `wp-init2`, worktree `/home/mcloonan/redoubt/.worktrees/init2`. Tip `d060d0782` (one WIP
  commit on `d974e247c`, wp-init1's tip). Not rebased: INIT1 not yet merged. No page commits yet.
- WIP commit holds all of deliverable 1 (fold before acceptance). Worktree clean except the fuzz
  run's outputs (ignored).
- Checkpoint not yet reported. Deliverable 2 waits for the go-ahead after it.

## Deliverable 1, as built (`servers/init`, package `redoubt-init`, lib only, no bin yet)
- `manifest.rs`: decode via `redoubt_rt::wire::json` (types, missing/unknown/repeat). The module
  doc's member table is the schema as built: devices{name,base(str),irq(num),dma}, labels{name,
  owner,id}, volumes{name,partition,labels}, servers{name,program,budget{pages(str),processes,
  weight},labels,devices[{device,as}],volume,receives,handed[{endpoint,badge(str)}],args},
  public[], principals{name,account(str),budget,ssh_keys,approval_keys,labels,label_sets[{labels,
  budget}],home "VOL:/PATH",net[{prefix,ports}]}, confined.
- `lib.rs`: `read(bytes, arena_pages)` refuses a manifest the arena cannot parse
  (`ARENA_PAGES`=256, json heap 32 B/byte, half the arena for the parse → 16 KiB max).
- `check.rs`: `check(&Manifest, &Machine, bundle_key) -> Result<Plan, Refusal>`, pure. Order:
  names → references (programs in bundle, labels/owners, volumes, handed endpoints received,
  badges, args NUL, principals' accounts/homes/prefixes/ports) → devices (match `device_info`
  answers by base/irq, DMA flag, split, one holder; returns placements NAME / NAME-irq) →
  budgets (processes, weight ≥ 1) → fit in `system` (pages + 1 budget page each, processes,
  weight) → public (entry exists, not `manifest`, once, needs a bootfsd) → blocks (builds each
  server's real StartupBuilder block: names, `/dev/cons`, args, image) → keys (R35 list, bundle
  key last) → buckets (ruling 2) → confine (if confined) → bound vs root free.
- `confine.rs`: R34; `cores()` is the isolated core rule (groups = distinct label sets > harts).
- `bound.rs`: pure `bound(&Counts)`: arena + its tables; (receives + one exit endpoint per server)
  × 1; process objects × 1; per server a block page + 3 tables; the largest single launch's
  transient copies (stub, image, stack, + tables each); handle-table pages beyond those in use
  (64 per page; added = endpoints + handed + 4 per server + 3 init caller handles).
  Image manifest's bound: 477 pages. Its buckets: keyd 1, consoled 1, bootfsd 1, ipd 1.
- `sshkey.rs`: `ssh-ed25519 BASE64` → 32 bytes, canonical base64, no crypto.
- `refusal.rs`: `Refusal` + `Why` + `Sharing`; Display gives the reason after
  `init: refused the boot: ` (line format pinned by a test).
- `fuzz.rs` (feature `fuzz`): QEMU virt fixture (`virt_devices`, `machine`, `ENTRIES`) and
  `check_one`; `fuzz/` crate with target `check`, seeds in `fuzz/seeds/check` (4 hand-written).
- `image/manifest.json`: the six-server manifest (devices: uart 0x10000000/10, disk
  0x10008000/8, net 0x10007000/7, as I believe QEMU places the first -device at the top slot:
  VERIFY on boot). Dev keyd seeds 11.., 22...
- Tests: `tests/manifest.rs` 29 + 8 unit, all green. Bench: `init-host-tests`, `init-build`
  (rv64, rv32) PASS. `tests/size-budget.toml` row servers/init 1045 (estimate; run `size-budget`
  to confirm the count). fmt applied to the crate (`cargo +nightly fmt -p redoubt-init`); fuzz
  crate not yet formatted from its own dir.

## Fuzz
- `cargo-fuzz` was not in the container; installed with
  `in-dev cargo install cargo-fuzz --locked --root target/tools` (in this worktree's target).
- A 1 h run started ~2026-10-02 in the background: corpus `target/fuzz-init/corpus` (≈1240 inputs
  at handoff), log `target/fuzz-init/run.log`, no artifacts so far. Run:
  `cd servers/init/fuzz && in-dev bash -c 'export PATH=/work/.worktrees/init2/target/tools/bin:$PATH; cargo fuzz run check ../../../target/fuzz-init/corpus -- -max_total_time=3600 -max_len=8192'`.
  Then minimise (`cargo fuzz cmin check ...` or `-merge=1` into seeds/check), commit the corpus,
  report the run length at the checkpoint. If the run died with the session, rerun it.

## Open questions: QA INIT2-manifest-rules (Architect; detail .wash/local/INIT2-questions-1.md)
Assumed meanwhile (each behind one function):
1. Handed badges: each handed item `{endpoint, badge}`, badge 1..FIRST_MINTED_BADGE-1, once per
   endpoint (`manifest::handed`, `check::references`).
2. R33: `root`/`system`/`users` reserved; a receives/handed item naming one → `Why::BudgetHandle`
   (`check::endpoint_name`).
3. Bundle key: orchestrator says do NOT copy it and do NOT touch the loader; if ruled into
   libs/signing, K16 moves it and init names it after the rebase. `check` takes `bundle_key` as a
   parameter for now.
4. R34 users and order: endpoint, volume, network, device, server, cores (`confine::check`).
5. Defaults: buckets only where args carry `buckets=`; init is a caller at keyd/consoled/bootfsd
   (`check::INIT_CALLS`, `check::callers`); bound counts transient launch copies; ssh-ed25519 form.
Not yet asked (deliverable 2): (a) bootfsd's args vs `public`: does init append the public names
to bootfsd's args (bootfsd reads names from args) or must the manifest repeat them? (b) rt's
`heap` is the global allocator on target (map_anon on demand), which does not fit ruling 1's
fixed arena; changing it is libs/rt (not owned). (c) rt has no `device_info` wrapper (libs/rt or
use redoubt-sys directly?). (d) read root's usage before taking the arena, so the arena is not
counted twice. QA INIT2-confined-cores still awaits the owner (no page line on cores until then).

## Reading done (do not redo)
SWARM implementer + staging sections, the brief, QA INIT1-design/own-budget/confined-cores,
init.md (all), devices.md, boot.md (argument block to "What init does"), budgets.md (interface
to budget_usage), consoled.md, the "Started by init" sections, keyd keys/messages/seeds/running,
serving R26, todo/server-bucket-counts, testbench rule F/case file/starting programs/servers'
cases/bundle files/data entries/size budget, wire strict JSON, objects.md cost table. Code:
log-server.rs, bundle.rs, keyd bin, server bins' handle names (keyd/consoled/bootfsd/blkd/netd/
ipd endpoints named after the server; uart/uart-irq, disk/disk-irq, net/net-irq; netd `ipd`, ipd
`netd`), rig.rs launch and start_network (badges INGRESS 3, ROOT 4, NETD_CLIENT 5), startup.rs
builder, admit.rs `buckets`, client launch.rs header, image/boot.toml, mkimage.

## What to do first
1. Check the fuzz run (or rerun), minimise into seeds/check, fmt the fuzz crate from its dir,
   run `cargo testbench size-budget` to confirm the row, fold into the WIP as clean commits.
2. Report the early checkpoint (member_update ≤2000 bytes: members as built, bound formula and
   477, buckets, tests and fuzz length, page gaps = the questions above).
3. Wait for the go-ahead and the QA answers; apply answers in their one function each.
