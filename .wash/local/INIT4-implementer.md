# INIT4: the net cases boot the real `init`, and the net rig goes

Tier A (the bench, plus a test-only feature in a DMA driver). Size L. Needs INIT3. Start from
main once INIT3 has merged; B7 has merged long before that. Run every cargo and bench command as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

## Context rules (read these first; context ran out four times on INIT2)

- **Don't read whole files.** Run `grep -n`, then Read a range.
  - `tests/net/src/rig.rs` is 753 lines. Read it one mode at a time, as you port that mode:
    `fn tcp`, `fn peer`, `fn pinned`, `fn attacks`, and the helpers each one calls.
  - `servers/init/src/bin/init.rs` and `check.rs` are 550 and 614 lines, and you should need
    neither.
- **Don't open `.wash/qa/*.md`, INIT2's or INIT3's reports, or other briefs.** This brief holds
  what they decided. If you must open a QA file, read it only up to its checkpoint comment:
  `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Use `cargo testbench --list | awk '{print $1}'`. Read boot logs and
  pcap files under `target/testbench` only through `grep` or `tail`.
- **Read a file right before you Write it,** and prefer Edit.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/INIT4-report.md`.
- **If you hand off, keep the handoff short** and end it with "what consumed my context". Your
  successor reads that handoff and this brief, not the reading list again.

## Reading list (only these)

- `docs/testbench.md`, these sections: "The servers' cases under `init`", "Disks and network
  cards" and "Peers, dials and the capture".
- `docs/servers/ipd.md`: "Sizing", for the arguments. Also `servers/ipd/src/args.rs`, its first
  20 lines: `self=`, `scope=BADGE:RULE`, `ingress=`, `limits=` and `buckets=`.
- `image/manifest.json`: the `devices` list and the `netd` and `ipd` entries. Each net case
  manifest starts from these.
- An existing servers' case manifest, `tests/init-servers.toml`, and the manifest it packs. Copy
  its `keyd` and `consoled` entries.
- `tests/net/src/rig.rs`, one mode at a time (see above), and `tools/testbench/src/qemu.rs`,
  only around `virtio-blk-device` and `virtio-net-device`.

## What moves, and what does not

The only boot cases that run real servers outside `init` are the net rig's, `netd` and `ipd`:
- `keyd`, `bootfsd` and `consoled` already run under `init` (INIT2).
- `blkd` is started by `init` in `init-boot` and had no rig. Its use on target is the fsd step's.
- `sshd`'s loopback cases run its host platform, and `bench-ssh-guest` checks only QEMU's
  forward. Both stay as they are until the sshd step.

**Keep every case's name and claim,** so the citations on ipd.md, netd.md, serving.md, init.md
and SECURITY.md stay good. Only its programs, and the manifest it packs, change.

Move them in this order:

1. **The bench's fixed slots.**
   - `qemu.rs` places the network card and the disk on fixed virtio-mmio slots, the ones
     `image/manifest.json` names: `net0` at base 268464128 (`0x10007000`), interrupt 7, and
     `disk0` at 268468224 (`0x10008000`), interrupt 8. They stay there whether or not the case
     has the other device.
   - Today the disk is added first, so the card's slot depends on whether there is a disk, and
     the rig found the card by probing.
   - Prove the placement with a host test, and in a boot through `init`'s device check: a wrong
     slot is refused at step 1 (`init-refuses-device-unmatched`'s rule).
2. **`net-tcp`.** The positive path: `netd` and `ipd` under `init`, one client, the peer
   round-trip. **Checkpoint:** send one progress line with the branch, this case's lines and its
   result on both widths, then go on.
3. **`bench-net-peer`** and its must-fail self-checks: `bench-net-peer-twice`,
   `bench-net-peer-count` and `bench-net-peer-pcap-empty`.
4. **`net-pinned`.**
5. **`net-attacks`**, and its must-fail twin `bench-net-self-unrefused` (the same manifest
   without its `self=`).
6. **`bench-virtio-legacy-off`** is a bench self-check about QEMU's transport, not a servers'
   case. It moves to a kernel case: `tests/programs`' `virtio-probe`, run by the tester in
   `init`'s place, which holds every device. It reads the card's version (2) at its fixed slot.
7. **`net-host-tests`.** The check that every net case forbids exactly the addresses its `ipd`
   refuses now reads each case's manifest: `ipd`'s `self=` list, plus `ipd`'s fixed refusals,
   must equal the case's `self_forbidden`. It also still parses each client's arguments
   strictly.
8. **`netd-restart`**, moved here from INIT3. See below.
9. **Delete the rig:** `tests/net/src/rig.rs`, the `net-rig*` binaries and the parts of
   `tests/net/build.rs` that embed images. The `tests/net` crate stays as the home of the net
   clients, the judge and the host test.

## What the rig gave, and what the manifest now gives

| The rig did | Under `init` |
| --- | --- |
| Found the card by its virtio device ID and its interrupt by slot | The case manifest's `devices` entry `net0`, by base and interrupt, at the bench's fixed slot (step 1). `netd`'s entry takes it `as` `net` |
| Launched `netd` and `ipd` with arguments and cross badges | Their entries, as in `image/manifest.json`: `netd` `client=5`, handed `ipd`'s endpoint with badge 3; `ipd` `addr=10.0.2.15/24`, `gateway=10.0.2.2`, `ingress=3`, `buckets=N`, handed `netd`'s endpoint with badge 5. Each case adds `self=`, `scope=` and `limits=` as its rig mode did (`SELF_ARGS` and each mode's rules) |
| Granted each client a scope at run time, in a budget of its own | **A `scope=BADGE:RULE` in `ipd`'s arguments, and that badge `handed` to the client's entry.** A client's labels are its entry's `labels`. A client that needs a narrower grant, or a disconnect by id, makes the `grant` and the `disconnect` itself, through its own badge. There is no steward stand-in: servers hold no budgets (R33), and the steward step builds dynamic grants |
| Collected clients' reports and exit codes, and printed `[net-rig] ok/FAIL` | **A judge** test program, the case's `reporter`. It receives on an endpoint the manifest names, and each client is handed a badge to it. The judge sequences the clients (the victim listens before the attacks) and prints `TEST PASSED` only when every check it owns passed. An attacker's report is information, never a verdict (rule F) |
| Kept no keys | Every manifest has `keyd` (`init` refuses one without it) and `consoled`; copy INIT2's servers'-case entries |

**Clients park; they never exit.** Under `init` an exit is a restart (INIT3), and a restarted
client would connect again, which the peer's count catches. A client reports its outcome to the
judge and parks.

**The network verdicts don't change:** the bench's peers, its dials and the capture, judged
outside the guest.

## `netd-restart` (from INIT3)

- **A test-only feature in `servers/netd`** (for example `restart-probe`), off in every default
  build, as the kernel's `dma-reset-deaf` is. With it, `netd` faults once, triggered from
  outside the guest: on a frame carrying a magic payload, sent by a bench dial or a peer
  exchange.
  - The trigger must not be a count inside `netd`, because the restarted instance would fault
    again and run into the reboot rule.
  - The `unsafe` budget and the size budget are measured on default builds: say so if the
    feature changes either.
- **The case:**
  1. A client round-trips with the peer.
  2. The trigger arrives, and `netd` faults.
  3. `init`'s lines appear: the fault, then `restarted netd, console N`.
  4. The kernel resets the card. No quarantine line may appear (`forbid`).
  5. `ipd` meets `unreachable` and retries.
  6. A second exchange is served: a second peer connection, counted, or a dial's echo.
- **Pages.** netd.md "Started by `init`": "restarts are not built" goes from its status, and the
  case joins its list. Its "A restart" paragraph stays unless the code differs; if it does, tell
  me where.

## Page lines (exact; each one in the commit that makes it true)

- **testbench.md**, "Disks and network cards": after "Devices use virtio-mmio's modern
  transport, which `blkd` and `netd` require.", add:
  > Each sits on a fixed virtio-mmio slot, the one `image/manifest.json` names: the card at
  > `0x10007000` with interrupt 7, the disk at `0x10008000` with interrupt 8, whether or not the
  > case has the other. So a case's manifest names its devices as the image's does.
- **testbench.md**, the table row for `tests/net/` becomes:
  > | `tests/net/` | the network clients and the judge that the net cases start under `init`, and the host test that checks each net case's manifest against its case file |
- **netd.md**, "Started by `init`": replace "The net rig (`tests/net/src/rig.rs`) does this in
  the bench, finding the card by its virtio device ID." with:
  > In the bench, a case's own manifest does the same, naming the card at the fixed slot the
  > bench gives it ([disks and network cards](../testbench.md#disks-and-network-cards)).
- **netd.md**, "Residual risks": the bullet "**`netd` does not boot under `init` in the bench.**
  …" goes.
- **ipd.md**: replace "The net rig (`tests/net/src/rig.rs`) does this in the bench." with:
  > In the bench, a case's manifest does the same: one `scope=` and one handed badge for each
  > client.
- **init.md**, "Launching through the loader stub": replace "The bench's launcher,
  `stub-launch`, and the net rig (`tests/net/src/rig.rs`, which stands in for `init` to launch
  the real `netd` and `ipd`) both launch this way" with "The bench's launcher, `stub-launch`,
  and `init` both launch this way".
- **init.md**, "The startup block" status: replace "in a boot, only the blocks the net rig and
  `stub-launch` write are parsed" with "in a boot, only the blocks `init` and `stub-launch`
  write are parsed".
- **Status lines**: each moved case keeps its place in every status list. A list loses only
  `bench-virtio-legacy-off`'s old program, and gains `netd-restart`.
- **The M1 page is mine.** Don't edit `docs/plan/m1-separation.md`; I write it when the `init`
  step closes (below).

## Owned paths

- `tests/net/**`.
- `tests/*.toml` for the cases named above, and their manifests.
- `tests/programs/src/bin/virtio-probe.rs`, if step 6 needs it.
- `tools/testbench/src/qemu.rs`, for the slots.
- `tools/testbench/src/case.rs`, only where a net case needs it.
- `servers/netd`: its `Cargo.toml` feature and the feature's code, nothing else.
- The page lines above.

**Not yours; ask first:**
- `tools/testbench/src/{main.rs,build.rs}` belong to B7 (per-run directories, packing). It will
  have merged, but its author's rules stand: ask before you change them.
- `servers/init`, `libs/rt`, `libs/client`, the kernel and `libs/sys` belong to RT1, K16 and
  INIT3's successors.
- INIT3 has merged before you start, so its `tests/*.toml` are on main. Change none of them.

## Gates

- The whole bench on both widths, alone (one whole bench at a time).
- `net-host-tests`.
- The testbench's host tests.
- `cargo fmt --check`.
- The size and unsafe budgets.
- doccheck.

Report each command with its exit code. The report lists:
- each case, with its old and new programs and its result on both widths;
- the lines deleted (the rig, and any testbench code that only served it);
- each page line, as written.

## What closes the `init` step (for the orchestrator; not INIT4's work)

The step needs INIT4 and `gate` (done). After INIT4 merges, I check:
- that init.md has no planned section left except those the steward, `sshd` and fsd steps
  own: step 6, blame, an `fsd` for each volume, and "A worked configuration";
- that the M1 page's "`init` and the boot manifest" item is wholly built. I then move it to
  Progress with exact lines, and correct the page's stale "Not built: `init`'s manifest
  handling" line and its attack-suite rows for R33 (built since INIT2) and R31/R32 ("from a user
  parent" stays the steward's).

The step closes when that page edit is on main.
