# FSD3: `fsd` under `init`

Tier A (it closes R47 and puts labelled volumes on the real disk). Size L. It needs FSD2 and INIT4
merged. Run every cargo and bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`,
from the worktree.

The file server step's last package. FSD1 built the server and FSD2 its quotas, both on the host.
This one runs `fsd` in a boot: `init` starts one per volume, `blkd` learns each range's labels, the
image carries a packed disk, files survive a reboot, and `fsd` restarts.

## Context rules (read these first; context ran out four times on INIT2)

- **Don't read whole files.** Run `grep -n`, then Read a range.
  - `servers/init/src/` is large. You need `manifest.rs`'s `Volume` and `Server`, `check.rs`'s
    volume checks, `confine.rs`'s volume rule, and the code that starts a server and repeats its
    boot steps at a restart (`grep -n "fn start\|fn restart\|handed"`).
  - `servers/blkd/src/server.rs`: only the label check (`grep -n NO_LABELS`) and its startup in
    `src/bin/blkd.rs`.
  - `tools/testbench/src/qemu.rs`: only `virtio_devices` and `gpt_disk`; `case.rs`: only `Disk`.
- **Don't open `.wash/qa/*.md`, other packages' reports or other briefs.** If you must open a QA
  file, read it only up to its checkpoint comment: `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Use `cargo testbench --list | awk '{print $1}'`. Read boot logs only
  through `grep` or `tail`: they begin with hex dumps.
- **One whole bench at a time.** Run sweeps in parallel, never two whole benches.
- **Read a file right before you Write it,** and prefer Edit.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/FSD3-report.md`.
- **If you hand off, keep the handoff short** and end it with "what consumed my context".

## Reading list (only these)

- `docs/servers/fsd.md`: "Volumes, connections and labels", "Quotas" (its bullets), "Authority",
  R47, "Failure and restart".
- `docs/servers/blkd.md`: "Ranges and badges" and "Started by `init`".
- `docs/servers/init.md`: "The boot manifest" (the `volumes` and `servers` rows), the confined
  rules (R34), and "Restarts and reboots".
- `docs/testbench.md`: the section on a case's `[disk]`, and how a reboot case ends at the next
  boot's first line (INIT3's cases).
- `image/disk.toml` and `image/README.md`.

## What is settled (cite these; reopen none)

- **One `fsd` per volume (R47)**, holding its endpoint, its one range and the connections it
  minted (fsd.md, Authority).
- **A range is a badge: GPT entry i is badge i + 1**, and `blkd` mints nothing and remembers
  nothing (blkd.md).
- **The manifest's `volumes`** (name, partition, labels) and a server's `volume` member, which
  INIT2 parses and checks but does not mint.
- **A volume `fsd` did not write is corrupt** (FSD1's mounting rule): every entry carries its id.
- **Restarts repeat the boot's own steps** (INIT3).

## The rules this brief settles

1. **`init` starts `fsd`.** For a `servers` entry with `volume`:
   - mint the range badge, the volume's partition entry + 1, at `blkd`'s endpoint, and hand it as
     the handle `volume`;
   - pass `labels=` the volume's label ids (absent when the set is empty) and the entry's
     `buckets=`;
   - a restart mints the range badge again, as a boot step.
   `fsd` starts after `blkd`, in manifest order, as every server does.
2. **`blkd` learns each range's labels from its arguments.** `init` passes one argument per
   labelled volume on that disk: `labels.P=ID[,ID...]`, P the partition entry number. `blkd`
   parses them strictly and exits with `BAD_ARGS` before serving on a malformed one, a P named
   twice, or a P that names no partition. A range named by none has no labels. `NO_LABELS` goes:
   the check takes the range's own set. `blkd` still remembers nothing: a restarted `blkd` gets
   the same arguments.
3. **Confined boots.** A confined manifest with a labelled volume must start: its `fsd`, carrying
   the volume's labels, writes through `blkd`. First check whether `init`'s confinement rules
   accept `blkd` serving that range. If they refuse every manifest with a labelled volume, stop and
   ask me: that is a rule to settle, not to work around.
4. **The image's disk.** `./image/mkimage` packs `image/disk.toml`: a GPT, then the `data`
   partition as a littlefs volume holding the staged files.
   - Write it through `fsd`'s own volume code, as a host function in `redoubt-fsd`, so every entry
     has its id and the counter is right. Never through a second writer.
   - The packed image must also mount in the C reference and read back the same tree: add a case
     to `libs/littlefs/diff/` (Rust writes, C reads), run it by hand (it is outside the gates),
     report the result, and remove its `target/`.
5. **Restart.** A restarted `fsd` mounts (with FSD1's and FSD2's mount checks) and serves. It runs
   no whole-volume block check first: R50 leaves a consistent volume after a power cut, and R49
   makes damage `corrupt` wherever a request meets it. This settles fsd.md's Open item.
6. **The reboot persists.** Files written before a reboot are there after it, on the same disk.
   Use the reboot INIT3's cases already follow, in one QEMU run with the disk kept. If the bench
   cannot carry a case past a reboot to a verdict, add that (B7 owns `tools/testbench/src/
   {main.rs,build.rs}`: ask before you change them), not a second QEMU process.

### Page lines (exact; each in the commit that makes it true)

- **fsd.md**, "Volumes, connections and labels": the status loses its "partly tested" clause; name
  the new bench cases.
- **fsd.md**, "Authority" and R47: built and tested, naming `fsd-one-volume`.
- **fsd.md**, "Failure and restart": built and tested. Replace "**Open:** whether a restarted
  `fsd` runs the volume check before serving." with:
  > A restarted `fsd` mounts and serves, with no whole-volume check first: a power cut leaves the
  > volume consistent ([R50](#r50-power-loss-leaves-before-or-after)), and damage is `corrupt`
  > wherever a request meets it ([R49](#r49-a-hostile-medium-is-corrupt-not-a-crash)).
  >
  > **Open:** none.
- **blkd.md**, "The label check" bullet: replace "against the range's labels" with "against the
  range's labels, which `init` gives `blkd` as arguments (`labels.P=ID,...` for partition entry
  P; a range named by none has none)". "Started by `init`" loses its "partly tested" clause.
- **init.md**: the boot sequence's status loses "an `fsd` for each volume" from its not-built
  list; in the `servers` row, after "volume", add "(its range badge, minted by `init`, and its
  label ids as `labels=`)".
- **image/README.md and disk.toml**: drop "not built yet" and the NOTE; say how `mkimage` packs it.

## The cases (both widths)

1. **`fsd-boot`**: `init` starts `blkd` and an `fsd` on a blank partition; `fsd` formats it; a
   client writes, reads, renames and removes through 9P.
2. **`fsd-reboot`**: the first boot writes a file and a directory; the box reboots; the second
   boot's client reads them back with the same qid paths.
3. **`fsd-confined-labelled`**: a confined manifest with a labelled volume; its `fsd` writes; a
   caller without the labels is refused at `fsd`.
4. **`fsd-one-volume`** (R47): two volumes, two `fsd`s. Each `fsd`'s startup holds only its
   endpoint and its range; a sector past its range is `out_of_range`; the other range's badge is
   not in its table. Under a test feature, an `fsd` that tries to reach anything else fails.
5. **`fsd-quota`** (R48 in a boot): two roots minted with quotas at one volume; one fills its
   quota; the other still writes.
6. **`fsd-restart`**: `fsd` is killed (a test feature triggered once, as INIT4's netd case is) and
   `init` restarts it; an old connection's call gets `Dead`, a fresh one reads the files.
7. **`fsd-corrupt-volume`**: a partition of noise is served as corrupt; `fsd` prints its line
   under `init` and stays up, and the boot goes on (no restart loop).
8. **`image-disk`**: a boot with `mkimage`'s disk serves the staged files.

## Owned paths

- `servers/init/**`: starting `fsd` and `blkd`'s arguments.
- `servers/blkd/**`: the arguments and the one check line.
- `servers/fsd/**`: the packing function, and fixes a boot finds (report each).
- `image/mkimage`, `image/disk.toml`, `image/README.md`, and the manifest's entries.
- `tests/fsd-*.toml`, `tests/image-disk.toml`, their programs.
- `tools/testbench`: only what rule 6 needs, asking first for B7's files.
- `libs/littlefs/diff/`: the Rust-writes-C-reads case.
- The page lines above.

**Not yours:** `libs/rt`, `libs/client/src`, the kernel.

## Gates

- `fsd-host-tests`, `blkd-host-tests`, `littlefs-host-tests`, and the case that runs `init`'s host
  tests (`grep -l redoubt-init tests/*.toml`).
- The whole bench on both widths, alone.
- `cargo fmt --check`; the size budget; the unsafe ratchet; doccheck.

Report each command with its exit code, each rule with its code and case, and each page line as
written.

## Checkpoint

When `fsd-boot` passes on one width, send one progress line with the branch.
