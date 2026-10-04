# INIT2: init, the manifest, its checks and the boot

Tier A (the most privileged process after the kernel, a console that attributes every line, the
bench's verdicts under `init`), size L. Needs INIT1 (accepted, merging after GATE1). It starts on
an override, on `wp-init1`'s tip (`d974e247c` today, the rebase of `45ac8cde5`), and rebases onto
main once INIT1 merges, before its first page commit and again before acceptance. K16 runs at the
same time; its boundary is under "Hotspots".

Every cargo and bench command runs as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from
the worktree.

## The package

After INIT1, the loader starts one program in `init`'s place, and kernel cases put a tester
there. INIT2 builds the real `init`:
- `init` reads the boot manifest from the bundle's pages and checks all of it.
- It starts the servers that exist, in the order of init.md "Starting the servers": `keyd` with
  the key-separation check, then `consoled`, then `bootfsd`, `blkd`, `netd` and `ipd`. Each is
  started through the loader stub, in a budget carved from `system`, with its devices placed by
  name. It then pushes the `public` entries to `bootfsd`.
- `consoled` prefixes `[con N] ` to every line written through a minted connection.
- The bench boots the real `init` for the servers' cases.
- `./mkimage` packs from `image/boot.toml`.

Not INIT2's:
- **fsd.** It does not exist yet. `volumes` entries are parsed and checked. A `servers` entry
  whose program the bundle lacks is refused, like any unknown entry.
- **The steward and `sshd`.** Step 6 of "Starting the servers" belongs to the steward step.
  `principals` are parsed and checked (key separation, confinement, bucket counts), but no
  session starts.
- **Restarts, blame and the reboot rule.** These are INIT3's. In INIT2, a server's exit is printed
  under its manifest name, and the server is not restarted.
- **The net cases under `init`, and deleting the net rig.** These are INIT4's.

## Rules first

- `.wash/SWARM.md`: "The implementer" and "Staging, commits and handoffs". Stage by path. Never
  use `git add -A`, `git commit -a` or `git stash`. Make small commits, and leave a clean branch
  at acceptance.
- `CONTRIBUTING.md`: Tests, Documentation, Formatting, Commits.
- The design is read-only. A gap is a blocking question to the Architect that names the page, the
  rule and the options.
- R33 (no server holds a system budget) has no exception. `init` has no mode for the bench, no
  test feature and no compiled-out test path. Kernel cases keep the tester in `init`'s place.

## Reading list, in order

1. QA `INIT1-design` (`.wash/qa/INIT1-design.md`): the cut and the owner's four decisions.
   - `device_info` over a loader list.
   - consoled prefixes, which are yours.
   - A tester in `init`'s place.
   - QA `INIT1-own-budget`: R33 is unchanged under `init`.
2. `docs/servers/init.md`, all of it.
   - Yours: "The boot manifest", "The confinement check", "Starting the servers" (steps 1-5),
     "The key-separation check", "Fresh connections per child", Authority, R33, R34 and R35.
   - Read only: "The startup block" and "Launching through the loader stub". These are built, and
     you use them.
   - Stay planned: "Restarts and reboots" (INIT3) and "A worked configuration" (the steward's).
3. `docs/kernel/devices.md`:
   - `device_info`;
   - "Which process gets which device" (yours).
4. `docs/kernel/boot.md`: "The loader loads only the kernel and `init`" (built by INIT1), and
   "What `init` does with the bundle" (yours).
5. `docs/kernel/budgets.md`: "Root, system and users" (INIT1's split), "The tree from the boot
   manifest" (yours), and `budget_usage`.
6. `docs/servers/consoled.md` "Started by `init`". Also each section titled "Started by `init`" in
   `bootfsd.md`, `blkd.md`, `netd.md` and `ipd.md`, and `keyd.md` "Seeds from the manifest" and
   "Running under `init`".
7. `docs/servers/serving.md` R26 (admission fairness), on root badges and buckets. Then
   `docs/todo/server-bucket-counts.md`.
8. `docs/testbench.md`:
   - "Rule F";
   - "Starting a case's programs" (INIT1's tester, for contrast);
   - "The servers' cases under `init`" and "Data entries for `init`" (yours);
   - "Bundle files".
9. `docs/servers/wire.md` "Strict JSON". The manifest is parsed with it, not with a new parser.
10. The code:
    - `tests/programs/src/bin/log-server.rs`: how INIT1's tester reads the bundle, spawns
      through the stub and badges by place. `init` starts children the same way, and
      `stub-launch.rs` shows the stub.
    - `libs/rt/src/startup.rs`: the startup block.
    - `libs/rt/src/server/admit.rs`: `buckets`.
    - `libs/client`: launch, grants and ns.
    - `servers/*/src/bin/*.rs`: how each server reads its block today. `tests/net/src/rig.rs`
      starts `netd` and `ipd` by hand; copy what it hands them.
    - `tools/testbench/src/build.rs` `bundle()` and `case.rs`.
    - `image/boot.toml` and `mkimage`.

## Rulings for this package (the Architect's)

1. **How `init` checks its own usage against `INIT_PAGES`.** This rules the item left open in
   earlier handoffs. budgets.md already says that `init` "reads its own usage and refuses a boot
   it cannot run in". It works like this:
   - `init`'s heap is a fixed arena, taken once at start. The parser and the checks run in it. A
     manifest that does not fit the arena is a manifest error.
   - After the checks, and before it creates anything, `init` computes a bound from the
     manifest's counts. The bound covers every charge its calls make to `root`, as objects.md's
     cost table gives them:
     - a page for each endpoint it makes;
     - a process object for each server (charged to the creator's budget, which is `root`);
     - each server's startup block and its handle table's growth;
     - the arena.
   - `init` reads `root`'s free pages (limit less usage) with `budget_usage` on slot 1. It
     refuses the boot, as it refuses any manifest error, if the bound is larger.
   - If a charge still fails during the boot, the bound has a bug. `init` refuses the boot then
     too (system-failure power-off) and never runs a partial boot.
   - The bound is one pure function with host tests. Its value for `image/boot.toml`'s manifest
     is reported at the checkpoint.
   - Attack case: a manifest that names enough endpoints to pass every other check but exceeds
     the bound is refused before any server runs.
   - Exact page lines, replacing the sentence "`init` reads its own usage and refuses a boot it
     cannot run in" in budgets.md "The tree from the boot manifest":
     > `init` works in a fixed arena, and before it creates anything it bounds what the manifest
     > will cost it in `root`: the endpoints it makes, a process object and a startup block for
     > each server, the handles it keeps and mints, and the arena. It reads `root`'s free pages
     > with [`budget_usage`](#budget_usage) and refuses the boot if the bound is larger; a charge
     > that fails later is a bug in the bound, and refuses the boot too, so no boot runs half
     > started.
   - In init.md "Starting the servers", step 1 gains: "... or if what the manifest will cost
     `init` does not fit in what `root` keeps for it
     ([budgets](../kernel/budgets.md#the-tree-from-the-boot-manifest))".
2. **What a server's bucket count is checked against.** The page says "the (account, label set)s
   the manifest routes to that server". The manifest does not say which servers a session reaches:
   the steward builds sessions, and it is not here yet. So `init` counts every declared domain at
   every shared server:
   - every principal's unlabelled set;
   - each label set the principal works under;
   - plus the root badges `init` mints at that server, one per system caller (R26: system
     callers get separate shares only from separate root badges).

   An over-count costs buckets. An under-count would bind, so this count holds whatever the
   steward later routes. Exact lines, replacing init.md "Sizing"'s second sentence and its last
   two sentences:
   > `init` refuses the boot unless N is at least the number of (account, label set)s the
   > manifest declares (each principal's unlabelled set and every label set it works under) plus
   > the root badges `init` mints at that server, one per system caller. It counts every declared
   > domain at every shared server, not only those a session will reach: an over-count costs
   > buckets, and an under-count would bind.

   `docs/todo/server-bucket-counts.md` and its SUMMARY line go in the commit that lands the
   attack case.
3. **The page reconciliations left from INIT1's merge.** These are ruled; they are not open.
   INIT1's branch already did the built half:
   - devices.md: the stale `map_device` and `device_info` sentences;
   - budgets.md: "Root, system and users";
   - boot.md: the built sections;
   - testbench.md: "Starting a case's programs".

   INIT2 owns the rest, each in the commit that builds it:
   - boot.md "What `init` does with the bundle" goes to built, and the figure's dashed edge
     becomes solid.
   - budgets.md "The tree from the boot manifest" goes to built:
     - Its first bullet repeats the built table, so cut it to what the table does not say: why
       the split is set in the kernel. Leave no process count in it.
     - "(at most 63)" becomes "(at most `system`'s process limit)", because `init` starts only
       servers, in `system`.
     - The figure's dashed edges become solid, except the principals' edge (the steward's).
   - devices.md "Which process gets which device" goes to built. The quarantine reboot is
     INIT3's, so the status is "partly tested".
   - The rest go to built, with "partly tested" where INIT3 or the steward owns a claim:
     - init.md: the sections listed above;
     - consoled.md "Started by `init`";
     - the "Started by `init`" halves in bootfsd.md, blkd.md, netd.md and ipd.md (restart
       claims stay INIT3's);
     - keyd.md "Seeds from the manifest" and "Running under `init`";
     - testbench.md "The servers' cases under `init`" and "Data entries for `init`".
   - `docs/SECURITY.md` rows for R33, R34 and R35.
   - `image/boot.toml`'s header comment is stale: the loader no longer starts entries in order or
     refuses `grants`. Fix it with the recipe.
4. **Refusals.** A manifest error is printed on the UART, which `init` maps itself until
   `consoled` starts. The machine then powers off with the system-failure status, before any
   other process runs. The one exception is the key-separation refusal, which comes after `keyd`
   alone has run. Every refusal case is judged by `init`'s refusal line and the power-off status.
5. **The bundle key** that `init` asks `keyd` about is the loader's verifying key. Take it from
   the same source constant the loader uses, never a copy of its bytes.
6. **The announce line.** `init` prints each child's connection id bare when it starts the child.
   Fix one format, and state it on consoled.md and testbench.md. The bench reads a reporter's id
   only from a bare line (rule F). A case shows that a program printing an `init`-like line comes
   out prefixed with its own `[con N]`.
7. **No core rule (the owner's decision, QA `INIT2-confined-cores`, pages at 1d109ac5f).** A
   confined boot separates servers, volumes, endpoints, networks and devices. The kernel and the
   cores stay shared. The confinement check is INIT2's. Its domains and order are ruled in
   `INIT2-questions-1-ruling.md` Q4.

## Owned paths

- New `servers/init` (`redoubt-init`): the manifest and its checks as a library with host tests
  and a fuzz target, plus the binary.
- `servers/consoled`: the prefixes. The `[con N]` attribution also needs host tests.
- `servers/keyd`, `bootfsd`, `blkd`, `netd` and `ipd`: only what reading their block under
  `init` needs. Ask before you change anything else.
- `libs/rt/src/server/admit.rs` only if the bucket rule needs it.
- `tools/testbench`: the servers' cases under `init` (`manifest`, `reporter` by manifest name),
  `build.rs` for `./mkimage`.
- `image/boot.toml`, `image/README.md`, `mkimage`, and `GETTING-STARTED.md` if its `./mkimage`
  lines change.
- `tests/`: the new cases and their programs.
- Pages: those in ruling 3, `docs/todo/server-bucket-counts.md` and `SUMMARY.md`.

## Hotspots: do not touch, or coordinate first

- **K16 (running now).**
  - Its paths are the kernel, the loader, `libs/sys`, `libs/layout`, `libs/paging`,
    `libs/stride`, the model and `libs/rt/fake`. INIT2 needs no change to any of them. If you
    find you do, ask first.
  - K16 changes `MAX_START_HANDLES` (64 to 128), `RECEIVED_SLOTS`, `MAX_LABELS` and `Pid`, which
    becomes 16-bit. Use the constants and types by name, never as literals. A PID that `init`
    prints is a `Pid`.
  - **Counts.** The program count on boot.md is gone: INIT1 replaced it with "one initial
    process". Every process count from here on is K16's. That covers budgets.md's "Root, system
    and users" table and figure, `MAX_START_HANDLES` in init.md's startup-block table, and
    boot.md's handoff record. INIT2 writes no process or handle count on any page, and states
    them through `system`'s limits. Both packages edit budgets.md and init.md, in different
    sections. Whichever merges second rebases.
- **STEWARD1** edits testbench.md ("The case file", a kind row, a self-check), and wire.md and
  steward.md (yours to leave alone). Rebase over it.
- **The loader's `verify.rs`**, and how it maps the bundle: read only.

## Deliverables, in order

Every case runs on rv64 and rv32.

1. **The manifest and its checks**, host-tested, with no boot yet. Strict JSON through wire's
   parser: types, names and unknown or repeated members. It also checks:
   - devices: one entry per device, `-irq`, the 60-byte limit, and matching against a list of
     `device_info` answers given as input, DMA flags included;
   - R33 budget grants;
   - the fit in `system` (pages, processes and weight, against a usage record given as input);
   - `public`: unknown entries and the manifest itself;
   - buckets (ruling 2);
   - R34 by every sharing kind on the page (no cores);
   - the list of keys to ask `keyd` about (R35);
   - the `INIT_PAGES` bound (ruling 1).

   It also needs a fuzz target for the parser and checks, and a kept corpus that reruns in host
   tests. **Early checkpoint here.**
2. **`init` boots.** Steps 1-5 on QEMU `virt`: UART lines, then `keyd` and the check, then
   `consoled` with the UART handed over (`init` unmaps it first), then the rest, then the
   `public` push.
3. **consoled's prefixes**, and the announce line (ruling 6).
4. **The servers' cases under `init`:**
   - A case boots `image`-style with all six servers. A test program, as a `servers` entry, reads
     a data entry through `/boot` and prints a verdict that the bench attributes through its
     `[con N]`.
   - Boot-refusal attack cases:
     - R33: a budget is granted;
     - R34: confined, with a shared server, volume, endpoint, network and device, one case each;
     - R35: a login key, and the bundle key, given to `keyd`;
     - devices: unmatched, split, `-irq`, DMA flag mismatch;
     - the servers do not fit in `system`;
     - too few buckets;
     - `public` names the manifest;
     - the `INIT_PAGES` bound is exceeded.
   - The forgery case from ruling 6.
5. **`./mkimage`** packs from `image/boot.toml` through the bench's builder, with a manifest of
   the six servers. A bench or host check confirms that the recipe's bundle boots to `init`'s
   last line.
6. **Pages**: ruling 3, each in the commit that lands its test.

## Gates

- `cargo testbench`: the whole bench, on both widths.
- rv32 builds of `init` and the servers.
- `cargo fmt --check`.
- The unsafe ratchet: `init` should need none. Any `unsafe` states its invariant.
- The size budget: `init` is new, so add its row with the reason.
- `cargo run -q -p redoubt-doccheck`: clean.

Report each command with its exit code.

## Early checkpoint

After deliverable 1 is committed and green, report with member_update (at most 2000 bytes):
- the manifest's members as built;
- the bound's formula, and its value for the six-server manifest;
- the bucket counts that manifest yields;
- the host tests and the fuzz run's length;
- anything on the pages that the code could not follow.

Wait for the go-ahead before deliverable 2.
