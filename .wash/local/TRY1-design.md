# TRY1 design note: `./launch --system`

Base: main 3206c43b4. Proposed branch `wp-TRY1` in `.worktrees/TRY1` (the convention the other
packages use); the brief says "project root", so confirm before I branch (question 1).

## 1. The machine: one case file, read by both the bench and launch

The machine is a new case, `tests/launch-system.toml`, copied from `steward-ssh-two-principals`
(the userland login case): `recipe = "image/boot.toml"`, `memory_mib = 1024`,
`[userland] recipe = "image/userland.toml"`, `[disk] recipe = "image/disk.toml"`, `[net] forward = [22]`,
with the steward/sshd expect lines and one `[[session]] user = "alice"` that waits for the prompt,
evaluates a marker and exits. Two differences from the steward case:

- **No test stage.** `[disk]` takes the recipe's own stages (`target/image/stage`,
  `target/image/vault`), with `home/alice` and `home/bob` made first as `./mkimage` makes them,
  so the owner gets the image's disk, not a test fixture.
- **`launch = true`** (new `Boot` field): the bench does not assemble the QEMU line itself; it
  takes the argv the launch path writes (section 5).

`testbench --run --system` loads that case file and assembles the machine with the functions
the bench already uses: `pack_case` → `prepare` (bundle, manifest with `pin_roots`), `builder.userland`
(the run's one userland pack), `qemu::virtio_devices` (disk, read-only userland disk, net). No second
packer, and a change to the case's machine is a change to launch's.

## 2. The dev key, for one boot only

- Default: `$REDOUBT_TMP/launch/id_ed25519{,.pub}` (`<repo>/.tmp`, git-ignored), made once per
  checkout by `ssh-keygen -t ed25519 -N '' -C redoubt-launch` if absent. `--key PUB` takes the
  user's own public key instead (the ssh line then omits `-i`, or names the private half if it
  sits beside the `.pub`).
- The bundle is packed fresh into the run's directory on every launch (as `--run` does today), so
  the manifest copy is per boot. A new `system_keys` step on the manifest's bytes, beside
  `pin_roots` in `build.rs`, replaces `principals[name="alice"].ssh_keys` with `[the key]`
  before signing (with the dev signing key, as every bench bundle). `image/manifest.json` and
  `tests/keys/` are untouched; alice's committed test key does not work on this boot, bob's does.
  A host test on the step: alice gets exactly the key, nothing else in the manifest changes.
- Nothing secret is committed: the private half lives under `.tmp` only.
- This edits the manifest's values in the host tool, not its format, init or the steward: Tier B.
  If you want the key appended rather than replacing alice's test key, say so.

## 3. The port forward

`-netdev user,id=net0,restrict=on,ipv6=off,hostfwd=tcp:127.0.0.1:2222-:22`: the case's
`forward = [22]` with the host port fixed (`--ssh-port`, default 2222) instead of a free one,
via a `ports` argument to `virtio_devices` (the bench keeps passing `None`: free ports). Bound to
127.0.0.1 only; `restrict=on` as in the bench, so the guest reaches nothing outside. Before
booting, launch binds 127.0.0.1:PORT once and refuses with "port 2222 is taken; --ssh-port N" if
it cannot.

## 4. What the owner sees

```
$ ./launch --system
==> building rv64 (4 harts, 1 GiB) ...
+ qemu-system-riscv64 -machine virt ... -nographic
Redoubt rv64, 4 harts, 1 GiB. This terminal is the serial console (alice's console session);
Ctrl-A X quits QEMU. Once the console prints "sshd: listening on port 22", from another terminal:

  ssh -p 2222 -i /home/.../redoubt/.tmp/launch/id_ed25519 alice@localhost

Host key: SHA256:<fingerprint> (ED25519), the image's development host key
```

The fingerprint is computed in testbench from keyd's `ssh_host` seed in the manifest it packed
(ed25519-compact, already a dependency), so it is the key sshd will present. Defaults: `--arch
rv64`, `--smp 4` for `--system` (the owner's slice; `--run` without `--system` keeps 1),
`--ssh-port 2222`. The disk is packed fresh each launch (nothing kept between boots: stated in
the docs; a `--keep-disk` is an extra). `--print-only` prints the QEMU line and the ssh line, and
does not boot.

`./launch` (bash) does two things, each under `scripts/q`:
1. `q run --cores 8 -- cargo run -p testbench -- --run --system ... --qemu-argv $REDOUBT_TMP/launch/<arch>.argv`:
   build, pack, print, and write the QEMU argv NUL-separated.
2. unless `--print-only`: `q run --cores $smp --name launch -- <that argv>`: a lease for the
   cores QEMU's harts use, the console on the terminal (q runs the command with the client's
   stdio). If no q daemon answers (`q ping`), it boots without a lease and says so.

The build lease is not held while QEMU runs. GETTING-STARTED.md "Run" gains "Try the system": the
walk-through (log in; `help()`; `ed`; `fm`; a context `alice.work@`; resize the window; Ctrl+\\;
Ctrl-A X), and the dev key / host key / fresh-disk notes. Summaries checked: README.md,
GETTING-STARTED.md, image/README.md, docs/testbench.md (the case), the M2 page.

## 5. The regression case: `launch-system`

Boots the exact argv launch writes. In the bench, a `launch = true` case calls the same Rust
function `--run --system --print-only` runs (in process, not the shell script, to avoid a q lease
nested inside the bench's own), with `REDOUBT_TMP` pointed at the case's log directory (so a key
is generated fresh, exercising `ssh-keygen` and the manifest step) and a free port as
`--ssh-port`. It then spawns that argv unchanged, `-nographic` included, with stdio piped (the
console is judged as every boot's), and runs the one alice session against the forwarded port
with the generated key, checking the host key against the printed fingerprint
(`StrictHostKeyChecking=yes`, known_hosts written from it). Verdicts: init's lines, sshd's
`login alice: session ...`, the steward's Login audit with alice's principal, the prompt, the
marker echoed (rule F: system lines, not the client's). rv64 at 4 harts in the first slice; rv32
listed once the slice works. `[net]` cases take the q net lock as the others do.

Question: the spec says "launch --system --print-only's exact command line"; I read that as the
same code path and the same argv, not running the bash script inside the bench (question 2).

## 6. The idle case: `launch-idle` (second part, after the first slice lands)

Same machine and same launch path, 4 harts, the kernel with `sched-trace` (checked build; the
ring prints at `system_reset`). Steps: log in as alice, wait for the prompt, mark, sleep 60 s
host time with nothing typed, mark, power off (the trace prints). Measures:

- **Wakeups per second:** from the kernel's trace, the count of thread wake/dispatch events (and
  timer interrupts) between the two marks' trace times, per budget and in total, divided by the
  window. The marks are tied to the trace by the console line the session's last send produces
  (the trace's time of the VM's dispatch nearest it) — or simpler, the window is the trace's last
  60 s before the reset, since nothing else runs after the idle. I'll take the simpler one.
- **Host CPU:** QEMU's utime+stime from `/proc/<pid>/stat` at both marks: CPU seconds per wall
  second (1.0 = one host core busy).

Ceilings (proposed, to be confirmed by a first measurement and then stated in the case and the
docs): whole system ≤ 100 wakeups/s, each budget ≤ 50/s, QEMU ≤ 0.10 host cores at idle.
If the measurement exceeds them I report the numbers and the busiest budgets rather than raise
the ceilings. It is a host-clock case: `--quiet` class (alone), checked kernel feature in the
case's `kernel_features`.

## Questions

1. Branch: `.worktrees/TRY1` on `wp-TRY1` from main, rather than the project root?
2. The regression case boots the argv the launch code path writes, in process, not `./launch`
   itself (nested q lease). OK?
3. `--smp 4` default for `--system`, and alice's key replaced (not appended) for the boot?
4. Fresh disk each launch for the first slice; `--keep-disk` later?
