# BEAM2: the userland disk, checked object by object, and the shell on the UART

Tier A (`init`'s manifest, the verification rule, beamlet's platform, the bench), with the shell's
Elixir Tier B. Size L. Needs BEAM1 and FSD3, both merged: start from main (75245a114 or later).
Run every cargo and bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from
the worktree.

## Context rules (read these first)

- **Don't read whole files.** `grep -n`, then Read a range. In `userland/otp/redoubt/src/bin/beamlet.rs`
  read `main` and the `Modules` type (how it reaches `bootfsd` today); in `src/lib.rs` only
  `load_module`, `load_app`, `run` and `limits`; in `userland/otp/vm/src/vm.rs` only
  `locate_module` and the `EMBEDDED` list; in `servers/init/src/check.rs` only `blkd` and
  `volumes`; in `servers/init/src/bin/init.rs` only the launch loop (`launch.handle`,
  `launch.namespace`) and where a volume's range badge is minted; in
  `tools/testbench/src/qemu.rs` only the device slots (`DISK_BUS`) and the `-drive` line.
- **Don't open `.wash/qa/*.md`, other packages' reports or other briefs.** This brief holds what
  they decided.
- **Pipe bench and cargo output;** boot logs through `grep` or `tail`. A cross-build of beamlet
  prints a lot: keep `tail -30` and the `error` lines.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/BEAM2-report.md`.

## Reading list (only these)

- `docs/userland/beamlet.md`: "The `Platform` boundary" (the `load_module` bullet), "beamlet on
  Redoubt", and the Open under "Natives" on where the Elixir modules live.
- `docs/userland/shell.md`: "The shell in a session".
- `docs/kernel/boot.md`: "Verified boot" and R15.
- `docs/servers/init.md`: "The boot manifest" (the `servers` row: `handed`; the Volumes bullet),
  "The confinement check", R34, and "Restarts and reboots".
- `docs/servers/fsd.md`: "Volumes, connections and labels" (`endpoint=NAME`, the "Read-only
  ranges" bullet).
- `docs/testbench.md`: "Disks and network cards" (the `[disk]` table, `recipe`, `--pack-disk`,
  the fixed slots) and "Bundle files" (a `[[file]]`'s `servers` merged by name).
- `docs/servers/bootfsd.md`: "Serving `/boot`". `image/README.md`, `image/disk.toml`, `mkimage`.

## What is already built (FSD3, INIT5, BEAM1), and what this package adds

Built: `fsd` takes `endpoint=NAME` and `buckets=N`, is given its volume's range (a `volumes`
entry's partition + 1, minted at `blkd`) and `labels=`; a range `blkd` reports read-only is
served read-only, every change refused; `blkd` takes `labels.P=` per partition and reports a
read-only device; `testbench --pack-disk RECIPE OUT` packs a GPT by `blkd`'s builder and each
partition as littlefs through `fsd`'s own code, and `./mkimage` packs `image/disk.toml` with it;
a case's `[disk]` boots a zeroed disk or a recipe; a path manifest's `[[file]]` may carry
`servers` entries merged by name. A launch moves an image into the child 64 pages at a time, so
`init`'s bound no longer grows with beamlet's image (INIT5). beamlet's heap and ETS limits are
each its budget / 16, `budget_pages=` is required, and the budget is at least twice the VM's own
use (BEAM1).

Not built, and this package's: more than one disk (`init` allows one `blkd`, its check and its
range minting assume "the one disk"; the bench attaches one disk, on bus 7); attaching a disk
read-only; packing objects named by their hash and `system.index`; beamlet reading modules from
an `fsd`; beamlet in the image's manifest.

## The settled design (owner's decision 5 of the shell plan)

Three stores: the signed boot bundle (the kernel, `init`, the servers, beamlet with what it embeds,
and `system.index`); a **read-only userland disk** (OTP, Elixir and Redoubt's own Elixir, one
object per module, named by its SHA-256); the data disk (FSD3's). The userland disk is bound to the
bundle by `system.index` and checked object by object by whoever uses it, so the file system in
between is not trusted and no new server is needed.

1. **The packer.** Every module beamlet loads by name after boot is compiled deterministically by
   the pinned toolchain, stripped, and staged as one file, `/<sha256 hex>` of its bytes. A new
   recipe, `image/userland.toml` (one littlefs partition, its stage that tree), is packed by the
   same `testbench --pack-disk` into `target/image/userland.img`: never a second writer. The same
   step writes `system.index` into the bundle's staged `/boot` and `public` names it: one line per
   module, `<module> <sha256 hex> <bytes>`, sorted by module, LF-terminated, nothing else. Two
   packs of the same inputs are byte-identical, disk and index. Which modules: the closure the
   shell needs at its prompt plus the applications they belong to; report the count and size
   against the plan's measure (an idle prompt's 102 modules, 1.03 MiB stripped).
2. **What stays embedded.** Only what the VM needs before it can read the disk (the preloaded
   modules and what `locate_module` needs to reach the disk). Everything BEAM1 read from `/boot`
   by name moves to the disk. Report the embedded list.
3. **Two disks.** The userland disk is its own virtio disk, `disk1` in the manifest's `devices`, on
   a fixed slot of its own (bus 5: `0x10006000`, interrupt 6; check it is free), attached
   read-only by the host (QEMU `readonly=on`), so nothing on the box can write it. It has its own
   `blkd`, which reports it read-only, and its own `fsd` (`endpoint=fsd:system`), which serves it
   read-only (fsd.md's "Read-only ranges"). A `volumes` entry gains `disk`, the name of the
   `servers` entry of the `blkd` that serves it: required when the manifest has more than one
   `blkd`, refused if it names no `blkd`. `init` mints each volume's range at its own `blkd`'s
   endpoint; every rule that said "the one disk" or "the one `blkd`" now holds per disk (one GPT
   entry per volume on its disk; no `handed` item at any `blkd`'s endpoint; in a confined boot
   each disk holds one label set's volumes and its `blkd` carries that set).
4. **`load_module` checks.** At start, beamlet reads `/boot/system.index` through `bootfsd` and
   parses it strictly; a malformed index stops the VM before it runs anything. beamlet is handed
   the userland `fsd`'s endpoint as a named handle (`fsd:system`), the way it is handed `bootfsd`
   today, and attaches to it. Looking a module up: its hash from the index, the object read whole
   from `/<hash>` there, the bytes hashed, and the loader given them only if they match. A
   mismatch, a missing object or a short read is a loud failure: the module does not load,
   `load_module` answers not found, and beamlet writes one line on the console naming the module
   and the reason. Nothing is retried and nothing falls back to another source.
5. **What it guarantees, exactly.** A module the system resolves *by name* is the one the signed
   bundle names, byte for byte. It does not stop code from running: `code:load_binary/3` still
   loads bytes a session holds, within the session's authority (beamlet.md's "a lookup, not a
   gate"). The rule extends verified boot to the userland's integrity, not to a code-signing gate.
   A program on the disk is checked the same way before launch; launching is BEAM4's, so this
   package states the rule for programs and BEAM4 builds it.
6. **A start module that fails the check parks the VM.** If the module beamlet was told to start
   cannot load, beamlet writes the reason on its console and waits, without exiting: a tampered
   disk must not become a restart loop that reboots the machine. Any other end of the VM is
   restarted by `init` under its restart rule, as BEAM1's page says.
7. **The shell on the UART.** The image's manifest gains the userland `blkd` and `fsd` and a
   beamlet entry on `consoled`'s UART console with `Redoubt.Shell` as its start module, handed
   `bootfsd` and `fsd:system`, with `budget_pages=` its budget (BEAM1: required; heap and ETS each
   budget / 16; the budget at least twice the VM's own use at the prompt, with the modules loaded:
   measure that use and report it). In this package the shell has the console only (files are
   BEAM3's, launching BEAM4's): the prompt reads, evaluates Elixir and prints. It runs with the
   manifest entry's grants and no login: it is the machine's local console until the steward and
   sessions exist.
8. **The shell as a restartable server.** It stays an ordinary manifest server under `init`'s
   restart rule, with the manifest's default limit. A crash in typed code ends an Erlang process,
   not the VM, so the VM ends only on the budget backstop; restarting it gives the console back, and
   a VM that cannot stay up reboots the machine, which fails closed. Point 6 keeps a bad disk out
   of that loop.
9. **Docs in the objects (the owner's open choice; default below).** By default the packer strips
   the `Docs` chunk with the rest, so `h/1` answers that no documentation is available, and the
   documentation can ship later as an optional package (M5's packages). **Switch:** if the owner
   chooses to keep docs, the packer keeps the `Docs` chunk and nothing else in this brief changes;
   report the disk's size both ways either way.

## Confined boots and the userland disk (ruled by the owner, 2026-10-03)

R34 refuses a labelled domain that reads a shared unlabelled volume, and the userland disk is one.
The owner ruled: **one read-only attachment per label set, and R34 unchanged.** No volume is
exempt. The image's manifest is not confined, so this package's own boots are unaffected.

- In a confined boot each label set that runs beamlet gets its own read-only attachment of the
  same userland image (its own device, `blkd`, volume and `fsd`, all carrying that set), so no
  volume, server or disk is shared and the check passes as it stands. Build: nothing beyond
  point 3, plus one host test in `init`, `confined_gives_each_label_set_its_own_userland_disk`: a
  confined manifest with an unlabelled and an {L} beamlet, each with its own userland disk, blkd
  and fsd, is accepted; the {L} beamlet handed the unlabelled `fsd:system` instead is refused
  (R34, the endpoint). Its cost: a virtio slot, a `blkd` and an `fsd` per label set; QEMU's
  `virt` has eight slots, three in use.
- **beamlet.md** (superseded by ruling 4 below), "beamlet on Redoubt", states it (no Open): "In a confined boot each label set
  that runs beamlet reads its own read-only attachment of the userland disk, through its own
  `blkd` and `fsd` carrying that set, so no label sets share the disk or its servers
  ([R34 (confined placement)](../servers/init.md#r34-confined-placement))." List the host test
  under R34 and the confinement check in init.md (each count + 1).

## The cases (both widths)

Build each case's manifest as `image-disk` does: `image/manifest.json` with the case's entries
merged by name (`[[file]]`'s `servers`), never a copy of it.

1. **`userland-boot`**: a boot with the userland disk; the shell's prompt on the UART; a typed
   `Enum.sum(1..10)` prints `55`. The modules came from the disk (a count line beamlet prints).
2. **`userland-flipped-byte`**: the case flips one byte in one object after packing (a module the
   case loads on demand, not at start). Typing a call to it: the console's mismatch line naming
   the module, the call fails, the prompt is still there, and nothing else loads in its place.
   Forbid `init: rebooting` and any `exited` line.
3. **`userland-bad-start`**: the start module's object flipped: beamlet's parked line, no exit, no
   restart, and the rest of the boot goes on. Forbid `init: restarted beamlet` and `init: rebooting`.
4. **`userland-missing-object`**: an index entry whose object is absent: the same refusal as 2.
5. **`userland-read-only`**: a test program handed the userland `fsd`'s endpoint tries to create
   and to write a file: both refused read-only, and the volume reads back unchanged.
6. **Host tests**: the index parser (sorted, one line per module, a malformed line refused whole);
   the check (match loads, mismatch and short read refused); the packer's determinism (two packs
   byte-identical, each object's name its hash); `init`'s `disk` rules (two `blkd`s, each volume's
   range minted at its own, a missing or unknown `disk` refused, a `handed` item at the second
   `blkd`'s endpoint refused); the confined test above; the bench's second disk on its slot,
   read-only.

## Page lines (exact text in the report; the Architect checks them)

- **boot.md**: a new rule after R15, **R75 (verified userland)**: a module the system resolves by
  name, and a program it launches from the userland disk, runs only if its bytes hash to the entry
  `system.index` in the signed bundle gives it; a mismatch, a missing object or a short read loads
  nothing and says so. Status: built for modules, partly tested: programs are BEAM4's. Its tests:
  cases 2-4 and the host check tests. One sentence on what it does not do (point 5).
- **SECURITY.md**: R75's row, in the register's form.
- **beamlet.md**: the `load_module`/`load_app` row in "beamlet on Redoubt" becomes the lookup of
  point 4 (index, object at the userland `fsd`, hash); the "a lookup, not a gate" bullet names R75
  for system modules; the Open under "Natives" on where the Elixir modules live closes: on the
  userland disk, one object per module, bound to the bundle by `system.index`, with only what the
  VM needs to reach the disk embedded (point 2). The section's status loses "modules are read from
  `/boot` unchecked". Point 6's parked start, one sentence. The confined-boot sentence (the
  section above), a stated line, not an Open.
- **shell.md**, "The shell in a session": "The shell's modules come from the boot bundle like the
  rest of the system's" becomes "from the userland disk, checked against the signed bundle
  ([R75](../kernel/boot.md#r75-verified-userland))"; the status becomes built for the UART console
  only, partly tested: files and launching are BEAM3's and BEAM4's, and sessions are the steward's.
- **packages.md**: one paragraph on where system code lives: the userland disk, immutable and
  bound to the bundle, apart from packages on the data disk.
- **bootfsd.md**: `/boot` carries `system.index`, the userland disk's table, signed with the rest.
- **init.md**: the `volumes` row and the Volumes bullet gain `disk`, and every "the one disk" /
  "the one `blkd`" becomes per disk (point 3); the confinement check's disk sentence says "each
  disk". R34 unchanged under the default.
- **testbench.md**, "Disks and network cards": the second disk (its key in the case file, its
  fixed slot, read-only), the userland recipe, and the byte flip a case may ask for; the slot
  sentence names both disks.
- **image/README.md** and `mkimage`'s header: the userland disk (`userland.toml`,
  `target/image/userland.img`) and `system.index`.
- If the owner keeps docs (point 9), packages.md's paragraph says the objects carry them.

## Owned paths

- `userland/otp/redoubt/**` (the lookup and the check, the parked start), `userland/otp/vm/src/vm.rs`
  only for the embedded list, the shell's start module if it needs one.
- `servers/init/**`: the `disk` key, its checks, per-disk minting, the confined test.
- `tools/testbench/**`: the second disk, read-only attachment, the userland pack and the flip.
- `image/**` and `mkimage`: the userland recipe, `system.index`, the manifest's new entries.
- The cases above and the pages above.

**Not yours:** `servers/fsd` and `servers/blkd` beyond a fix a boot finds (report each); the
kernel; the natives (BEAM4); files (BEAM3); R34's check.

## Gates

- The whole bench on both widths, alone.
- The host tests of beamlet-redoubt, init, fsd and the testbench; the Elixir differential if the
  shell's modules change.
- `cargo fmt --check`, the size and unsafe budgets, doccheck.

Report each command with its exit code, the module count and the disk's size (stripped, and with
docs), the embedded list, the VM's own use at the prompt and the budget you gave it, each case's
verdict, and each page line as written.

## Rulings during the build (architect-12)

1. **Whole applications.** The disk holds every module of each application the idle prompt's
   closure touches (the plan's seven, 577 modules, 3.75 MiB), not the 102: a name looked up later
   must resolve. The recipe names applications; the packer stages each ebin whole. Report both
   measures. Page line (packages.md's paragraph): "whole applications, every module of each one
   the shell's prompt uses".
2. **`.app` files are objects, and the index is keyed by file name.** Each line is
   `<file> <sha256 hex> <bytes>`, the file name the VM asks for (`Elixir.Enum.beam`,
   `elixir.app`), sorted byte-wise; `load_module` looks up `<module>.beam`, `load_app`
   `<app>.app`, both checked the same way. (Module names do have dots: `Elixir.Enum`.) R75's text
   says "a module or application resource"; bootfsd.md states the line format.
3. **Bench and image keys: accepted,** with two conditions. The case's `[userland]` (recipe,
   optional `flip`/`remove` naming a file) changes the disk only: the index in the bundle is the
   unchanged pack's. `--pack-disk` writes an index only when the recipe declares one
   (`index = "system.index"` in `userland.toml`), so `disk.toml` packs as before. testbench.md
   ("Disks and network cards", "Bundle files") and image/README.md say so.

## Rulings during the build (architect-14)

4. **The confined index: closed by the owner, 2026-10-03.** BEAM2 lands as built. VOL1 (a block
   verifier, `.wash/local/VOL1-implementer.md`) follows and replaces the index. Until then:
   - The index stays `/boot/system.index`, read through `bootfsd` as built.
   - Cases 1-5 proceed as built.
   - The confined host test stays as built: each label set gets its own disk, `blkd` and `fsd`,
     and no beamlet is handed `bootfsd`.
   - **Replaced:** the beamlet.md confined sentence in "Confined boots and the userland disk"
     above. Write this one line instead, in "beamlet on Redoubt": "A confined boot runs no
     labelled beamlet: beamlet reads `system.index` through `bootfsd`, one instance a labelled
     domain may not share with the unlabelled ones ([R34 (confined placement)](../servers/init.md#r34-confined-placement))."
     VOL1 replaces it with the per-label-set rule.
   - **Nothing else in this brief changes.**
5. **Memory, erts and bench time (2026-10-03).**
   - **RAM: option (a).** beamlet's budget is 24,576 pages on both widths, which keeps BEAM1's
     rule (twice the measured 11,877 on rv64). Every case that boots the image's manifest sets
     `memory_mib = 1024`.
     - Not (b): it breaks BEAM1's rule.
     - Not (c): point 7 puts the shell in the image.
     - The split stays a quarter (budgets.md:126). The shell sits in `system` only until
       sessions exist, then each session's VM is carved from `users`. Growing `system` for an
       interim manifest shell would shrink the principals' share for good.
     - The measure does not follow the budget. `limits` caps one process's heap and ETS and
       reserves nothing (lib.rs:307-316), and at 8,192 pages (32 MiB) the VM died at its first
       line, so its need is above that whatever the budget.
   - **erts whole: confirmed under ruling 1.** `prim_file` makes erts an application the
     prompt's closure touches. One source per module: anything beamlet embeds is excluded from
     the disk with the recipe's `exclude`. Report the excluded list and the final count.
   - **One shell boot, every whole run.**
     - `userland-boot` carries cases 2 and 4. Its recipe flips one object and removes another,
       both loaded on demand. Type a call into each, check its refusal line, then
       `Enum.sum(1..10)` gives `55`.
     - Cases 2 and 4 are not separate cases.
     - `userland-bad-start` and `userland-read-only` stay separate. They do not wait for a prompt.
     - All of them run in the whole run (no `whole_run = false`). Give `userland-boot`
       `timeout_secs` at twice its alone time under the whole run, and report both times.
