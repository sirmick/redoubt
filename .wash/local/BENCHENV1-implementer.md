# BENCHENV1: OpenSSH's reference server runs in a QEMU guest, not a container

Tier A (the bench), size M, no needs. Worktree `.worktrees/BENCHENV1` on `wp-BENCHENV1`, from
`main` 9212f6f60. Every cargo and bench command runs as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

## The owner's decision (2026-10-04)

> can we run up openssh on a minimum qemu? that way no root needed at all. and we already use
> qemu as the main riscv64. [...] yep, go for it.

This replaces the earlier "keep rootless; supply a runner" answer on QA
`BENCHENV1-container-permissions`. Do not read that thread or the older `BENCHENV1-*` files in
`.wash/local/`: they record attempts to run the *container*; this brief supersedes them.

## What the reference case needs, and keeps

`docs/testbench.md` "Sessions and the loopback server": with `server = "openssh"`, `ssh`'s
`ProxyCommand` starts, for each session, a fresh pinned OpenSSH `sshd -i` with its stdio piped,
given the case's `sshd_config`, `authorized_keys` and host key, appending to one `sshd.log` per
case that `server_log` and `server_log_forbid` read afterwards. Nothing listens on a port; the
server has no network; it is a witness independent of Redoubt's `sshd`; before the first case the
bench logs in once and the log must name OpenSSH's version (`REFERENCE_VERSION`). Every one of
those properties stays. The container was a workaround for SELinux moving login shells
(`testbench.md:717`); a guest has no such host.

Today `tools/testbench/src/ssh.rs` does this with `podman run -i --rm --network=none` of an image
built from `tests/ssh-reference/Containerfile` (`Reference`, `proxy`, `ensure_image`,
`loopback_usable`, `CASE_FILES`, `BUILD_SOURCES`). This host cannot run rootless `podman` (user
namespaces are blocked in the dev container), so the reference case has failed on every bench
since, and three packages wait on it.

## The design

**One minimal Linux guest per SSH session**, started by `ProxyCommand`, under the
`qemu-system-riscv64` the bench already requires. The case directory exists before the
`ProxyCommand` runs (`loopback()` writes it), so nothing is shared or exported ahead of time.

The proxy, in the shape of today's `proxy()` (one string, `plain()`-checked, QEMU's stderr
dropped because it is `ssh`'s):

```
exec qemu-system-riscv64 -M virt -m 128M -smp 1 -run-with exit-with-parent=on
  -display none -monitor none -nic none
  -kernel <guest>/vmlinuz -initrd <case_dir>/initrd.img -append "<console and quiet flags>"
  -chardev file,id=con,path=<case_dir>/guest.log  -serial chardev:con
  -device virtio-serial-device
  -chardev stdio,id=io,signal=off               -device virtserialport,chardev=io,name=io
  -chardev file,id=log,path=<case_dir>/sshd.log,append=on -device virtserialport,chardev=log,name=log
  2>/dev/null
```

- **Stdio** is a virtio-serial port, a raw character device in the guest (not a tty), so the
  SSH byte stream is untouched. The guest's init runs
  `sshd -i -f /case/sshd_config -E /dev/virtio-ports/log </dev/virtio-ports/io >/dev/virtio-ports/io`,
  then powers off, so QEMU exits and `ssh` sees EOF. If `ssh` dies first it HUPs its proxy, and
  `exit-with-parent` is the second line.
- **The log** is written on the host by QEMU's `file` chardev with `append=on`: one `O_APPEND`
  writer per session on one file, so concurrent sessions' lines interleave whole, as the bind
  mount's did. The guest writes no host file.
- **The case's files** reach the guest in a small cpio appended to the base initramfs: the bench
  writes `<case_dir>/initrd.img` = the built base image + a `newc` archive of
  `/case/{sshd_config,authorized_keys,loopback-host}`. The guest therefore has *no* host
  filesystem at all (no 9p, no disk). Alternative if the copy per session proves a problem: a
  read-only 9p share of the case directory (`-fsdev local,readonly=on`); say which you built and
  why. The kernel console goes to `guest.log` in the case directory, for people reading a failed
  run; it is never a verdict.
- **The guest image** is built once per recipe, as the container image was, at
  `target/ssh-reference/<hash>/{vmlinuz,initrd.img}`, where `<hash>` is the first 16 hex digits
  of the recipe's sha256. The recipe `tests/ssh-reference/guest.toml` replaces the Containerfile:
  Debian trixie riscv64, a snapshot.debian.org timestamp, and for each package its name, version,
  URL and sha256: the kernel (`linux-image-*-riscv64`), `openssh-server` 1:10.0p1-7+deb13u4 (the
  current pin) and the libraries it links, `busybox-static` for `/init` and `/bin/sh`. Building:
  fetch each `.deb` (`curl`), check its sha256 (a mismatch is `Unusable::Broken`; a source that
  does not answer is `Unusable::Host`, as `BUILD_SOURCES` does today, now naming
  `snapshot.debian.org:443`), `dpkg-deb -x` into a staging root, add `/init`, `/etc/passwd` and
  `/etc/group` (root, and the `sshd` privilege-separation user), `/run/sshd`, the kernel modules
  the guest needs if they are not built in (virtio-mmio, virtio-serial/console; decompress `.ko.xz`
  with `xz` at build time if the kernel cannot), and write the `newc` cpio **from Rust** (the dev
  image has no `cpio`; the format is a few header fields and a trailer; a host test checks it).
  The guest boots with QEMU's bundled OpenSBI (`/usr/share/qemu/opensbi-riscv64-generic-fw_dynamic.bin`):
  the "RustSBI only" rule is Redoubt's boot, and this guest is not Redoubt; say so on the page.
  Pinning by snapshot removes today's residual (Debian drops superseded versions from its archive).
- **Nothing else changes**: `loopback()`'s `sshd_config`, `REFERENCE_USER` root, the probe in
  `loopback_usable`, the version check, `--allow-skip` semantics (no `qemu-system-riscv64`, or no
  network to build the guest, is a host lack; everything else fails the case), the sessions, the
  verdicts.

Dev image facts (checked 2026-10-04): `qemu-system-riscv64` 11.0.2 with `virtio-9p-device`,
`virtserialport`, chardevs `stdio`/`file`/`pipe`/`socket`; `dpkg-deb`, `curl`, `xz`, `sha256sum`,
`readelf` (reads riscv64 ELF), `gzip` present; **no** `cpio`, `busybox`, `qemu-system-x86_64` or
`/dev/kvm`. 24 cores, 60 GiB. Members install nothing; report what is missing.

## Checkpoint 1: the guest boots and serves one login (report before touching the runner)

By hand, in the worktree, with the recipe resolved (every URL and sha256 real): build the image,
boot it with the command above against a case directory you write by hand, and complete
`ssh -o ProxyCommand=... root@loopback exit 0` with the version in `sshd.log`. Report: the recipe,
the image's size, **the time from QEMU's start to sshd's first log line** (TCG, no KVM), which
drivers were modules, and anything the design above got wrong. Stop and report if the boot takes
more than ~20 s, if virtio-serial cannot carry the stream (fallback: the 16550 UART in raw mode,
`stty raw -echo cs8`), or if a pinned package is not in the snapshot.

## Deliverables

1. `tests/ssh-reference/guest.toml` and the builder (recipe parsing, fetch, verify, extract,
   `/init`, cpio) in `tools/testbench/src/ssh.rs` or a new `ssh_guest.rs`; the Containerfile and
   `Reference::{podman,sg}`, `group`, `CASE_FILES` go.
2. The proxy, `loopback()` writing the per-case `initrd.img`, `loopback_usable` building the guest.
3. **Keeper:** after a case's sessions all exit, no `qemu-system-riscv64` whose command line
   names that case directory is still running (scan `/proc/*/cmdline`; the directory's name is
   unique to the run); if one is, the case fails and names it.
4. **Host tests:** `the_reference_proxy_quotes_only_what_the_bench_chose` rewritten for the new
   command; the cpio writer (headers, padding, trailer; a two-file archive's bytes); the recipe's
   hash and parse; a sha256 mismatch is `Broken`, an unreachable source is `Host`.
5. `tests/bench-ssh-loopback-openssh.toml`'s description: "in a QEMU guest", not "in a container".
6. Pages, below.

## Page lines

`docs/testbench.md` "Sessions and the loopback server": line 711 "inside a container" → "in a
QEMU guest"; replace the paragraph "**OpenSSH's server runs in a container.**" and its five
bullets with one on the guest: why (the host's `sshd` cannot serve it under SELinux, and a
container needs a runtime and user namespaces the bench's host may refuse; a guest under the
bench's own QEMU needs nothing), the image (the recipe, its pins by snapshot, the hash tag, built
once, the one step needing the network), each session (its own guest, the case's files in its
initramfs, stdio on a virtio-serial port, the log appended on the host by QEMU, no network, no
host filesystem, QEMU's OpenSBI), the probe and `--allow-skip` (what is a host lack now), and the
keeper. Lines 791-794 "Its server runs in a container" likewise. Keep the status lines' test
names accurate. `GETTING-STARTED.md`: the prerequisites already name `qemu-system-riscv64`; add
that the reference guest's first build needs the network once. Grep `podman|container|Containerfile`
across `docs/`, `GETTING-STARTED.md`, `Dockerfile`, `dev.sh` and `tools/` and report every hit
with its disposition (`docs/servers/{keyd,pkg}.md` and `docs/kernel/boot.md` use "container" in
other senses: check, do not edit blindly).

## Owned paths

`tools/testbench/src/ssh.rs` (and a new `ssh_guest.rs` if you split it), `tests/ssh-reference/`,
`tests/bench-ssh-loopback-openssh.toml`'s description, `docs/testbench.md`'s loopback section,
`GETTING-STARTED.md`'s prerequisites. Nothing in the kernel, servers or runtime. Hotspots: none;
`wp-BEAM7`, `wp-MEM1` and `wp-SCHED1` do not touch these files.

## Gates (Tier A, the bench)

- `cargo testbench bench-ssh-loopback-openssh` five times, then the whole bench on both widths,
  **with no `--allow-skip`**: the reference case must PASS, not skip. One whole bench at a time
  on this host; nothing else building while it runs.
- The testbench host tests; `cargo +nightly fmt --all --check`; doccheck; the size budget's bench
  row if it has one; `git diff --check`.
- Report each command with its exit code, the guest's boot time, and the whole bench's counts.

## Context rules

Tail bench and cargo output to the verdict and failing lines; never read a whole log, a QA file,
or a boot log's hex. Read `ssh.rs` once, in full, since you commit it; read `testbench.md` only
the loopback section and the prerequisites. Report with `member_update` (≤ 2000 bytes, detail in
`.worktrees/BENCHENV1/target/BENCHENV1-report.md`), set waiting, end your turn. At 70 % context,
stop at a safe checkpoint, commit by path, register a handoff. Never push; never `git add -A`.
