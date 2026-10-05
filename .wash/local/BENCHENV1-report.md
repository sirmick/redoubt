# BENCHENV1 checkpoint 1: the guest boots and serves logins

Done by hand in `target/cp1/` (not committed). Branch `wp-BENCHENV1` unchanged (9212f6f60);
no runner changes. Nothing installed. All commands ran in the dev container via `in-dev`.

## Result

`ssh -F /dev/null -T -l root -i alice ... -o ProxyCommand=<qemu> -- loopback 'exit 0'` → exit 0,
empty stderr, `sshd.log` has `debug1: sshd-session version OpenSSH_10.0, OpenSSL 3.5.7 9 Jun 2026`
(and `sshd-auth version OpenSSH_10.0`), so `REFERENCE_VERSION` matches unchanged. Also checked:
- `-tt` session: `tty` prints `/dev/pts/0`; `echo hi-$((6*7))` → `hi-42`.
- key `mallory`: `root@loopback: Permission denied (publickey).`, ssh exit 255; log has
  `Failed publickey for root`. Two sessions' lines append whole to one `sshd.log`.

## Timing (TCG, no KVM; QEMU start → first byte of sshd.log / → ssh exit)

- 5 sequential: 1406–1419 ms / 1605–1635 ms.
- 3 rounds × 4 concurrent: 1461–1497 ms / 1693–1730 ms.
- With the 17.8 MB image at -m 128M: 1371–1386 ms / 1592–1601 ms.
The kernel reaches power-down at ~0.97 s guest time when sshd closes first.

## Recipe (draft: `target/cp1/guest.toml`)

Snapshot `https://snapshot.debian.org/archive/debian/20261001T000000Z/`, trixie main riscv64
(`dists/trixie/main/binary-riscv64/Packages.xz` there). All 24 debs fetched from
`<snapshot>/<Filename>` and matched the index's sha256:

| package | version | sha256 (first 16) |
| --- | --- | --- |
| openssh-server | 1:10.0p1-7+deb13u4 | 06f57163981c9043 |
| busybox-static | 1:1.37.0-6+b9 | see guest.toml |
| linux-image-6.12.107+deb13-riscv64 | 6.12.107-1 (112 MB deb) | see guest.toml |
| libc6 2.41-12+deb13u4, libssl3t64 3.5.7-1~deb13u2, libaudit1, libaudit-common, libcap-ng0, libcom-err2, libcrypt1, libgssapi-krb5-2, libkrb5-3, libk5crypto3, libkrb5support0, libkeyutils1, libpam0g, libselinux1, libpcre2-8-0, libwrap0, libwtmpdb0, libsqlite3-0, zlib1g, libzstd1, libgcc-s1 | per guest.toml | per guest.toml |

The library set is the `lib*`/`zlib1g` closure of openssh-server's Depends. `readelf` confirms
it covers every NEEDED of `sshd`, `sshd-session` and `sshd-auth`. openssh-server 1:10.0p1-7+deb13u4
riscv64 has been in the archive since 20260507T023346Z.

## Image

- `vmlinuz`: the deb's `boot/vmlinux-6.12.107+deb13-riscv64`, a raw RISC-V `Image`, 31.2 MB.
- Base initramfs (uncompressed newc): 23.9 MB as extracted minus `usr/share` and the unit files
  in etc; **17.8 MB** after also dropping `usr/lib/riscv64-linux-gnu/gconv` (6.4 MB, unused).
  It compresses to 11.2 MB with gzip and 7.4 MB with xz. I kept it uncompressed: each session's
  copy is cheap and TCG does not have to decompress it.
- Added: `/lib`→`usr/lib`, `/bin`→`usr/bin`, `/sbin`→`usr/sbin` (trixie is merged-usr and ships
  no such links in these debs; `PT_INTERP` is `/lib/ld-linux-riscv64-lp64d.so.1`), `/usr/bin/sh`→busybox,
  `/etc/passwd` (root `/root` `/bin/sh`; `sshd` uid 100), `/etc/group`, `/run/sshd`, `/root` 0700,
  `/case`, `/dev/console` (char 5,1: newc carries device nodes without root; without it init has no
  console), `/init`, `virtio_mmio.ko`.
- Case archive: `case/{sshd_config,authorized_keys,loopback-host}`, uid 0, the host key 0600
  (sshd refuses a group- or world-readable key even with `StrictModes no`). It is appended to the
  base, and the kernel unpacks both.

## Drivers

Built in: virtio core, `VIRTIO_CONSOLE` (virtio-serial), 8250/16550 console, devtmpfs, devpts,
initramfs gzip/xz/zstd, syscon poweroff. **Module: `virtio_mmio` only** (`depends=` empty),
decompressed from `.ko.xz` with `xz` at build time and loaded with busybox `insmod`. The kernel has
`MODULE_DECOMPRESS=y` too, but busybox insmod does not ask for it. Not needed: virtio-rng (the
CRNG was ready in time with no entropy device).

## What the design got wrong or left out

1. **A virtio-serial port takes one open at a time** (`EBUSY` on a second). The io port
   needs `<>/dev/virtio-ports/io >&0`, not separate `<` and `>`.
2. **`-E /dev/virtio-ports/log` fails**: sshd, sshd-session and sshd-auth each reopen the `-E`
   path, so the second open gets `EBUSY` and sshd-auth exits 1 before authentication. `-e` with
   stderr on the port is wrong too: the session child's `debug1: permanently_set_uid` then reaches
   ssh's stderr, which is session output. **Built instead:** init makes a FIFO `/run/sshd.log`,
   `cat`s it to the log port, holds a writer open (fd 3) so the reader sees EOF only at the end,
   and runs `sshd -E /run/sshd.log`. Writes under PIPE_BUF stay whole, and the host still has
   QEMU's one `append=on` writer per session.
3. **There is no `/dev/virtio-ports/<name>`** without udev. Init links them from
   `/sys/class/virtio-ports/*/name`, and must loop until both names appear: a port's `name`
   arrives a little after its sysfs directory (the third boot I ran raced, hung waiting, and needed killing).
4. **Who exits first is the other way round.** After `exit-status` OpenSSH's client exits without
   waiting. sshd waits for an EOF that a virtio port never delivers, so the guest does not power
   off. ssh's HUP of its proxy and `exit-with-parent` end QEMU within about 1 s. The guest powers
   itself off only when sshd closes first (a refused key, say). Residual, as with the container:
   sshd's lines after ssh has gone are lost. The `server_log` lines the cases use come earlier.
5. **`-no-reboot` and `panic=-1` should be added**, so a failed `/init` (kernel panic) ends QEMU
   at once instead of rebooting the guest until the case times out. Append used:
   `console=ttyS0 quiet panic=-1`.
6. **Memory: -m 128M is not enough for the 23.9 MB image** ("Initramfs unpacking failed: write
   error"). The 17.8 MB image works at 128M. I propose `-m 256M`, since QEMU commits guest RAM
   only when the guest touches it, and dropping `gconv` (a recipe `omit` list: `usr/share`,
   `usr/lib/riscv64-linux-gnu/gconv`).
7. **`guest.log` on a `file` chardev without `append=on`** truncates. A case's concurrent
   sessions would clobber one another's consoles. Use `append=on` there too.
8. `busybox --install -s /usr/bin` at boot supplies `tty` and the other commands the cases run
   (sshd's PATH is `/usr/bin:/bin:...`). This saves listing applets in the recipe.
9. Dev-container note: its PID 1 (bash) does not reap orphans, so exited QEMUs stay zombies there.
   The keeper's `/proc/*/cmdline` scan must ignore zombies (their cmdline is empty, so it does).

## Next (after the orchestrator's go)

Builder in Rust (recipe parse/hash, curl fetch, sha256, `dpkg-deb -x`, omit, links, init,
passwd/group, `/dev/console`, xz -d of the module, newc writer), proxy, per-case initrd, keeper,
host tests, pages, then the gates.

---

# Step 2: the Rust side (assignment cc789a4f)

Commits on `wp-BENCHENV1` (base main 9212f6f60):
- 2f38e14d1 `testbench: OpenSSH's reference server runs in a QEMU guest`. It adds
  tests/ssh-reference/guest.toml (new), tools/testbench/src/ssh_guest.rs (new) and
  tools/testbench/src/ssh.rs, and changes tools/testbench/src/main.rs (`mod ssh_guest;`, plus one
  comment changed from "container" to "guest"; outside the owned paths). It deletes
  tests/ssh-reference/Containerfile and updates the description of
  tests/bench-ssh-loopback-openssh.toml.
- f7a0cb1eb `docs: OpenSSH's reference server runs in a QEMU guest`, covering docs/testbench.md
  and GETTING-STARTED.md.

## The eight checkpoint-1 fixes, where they live

1. io port: init's `<>/dev/virtio-ports/io >&0` (guest.toml `init`).
2. A FIFO `/run/sshd.log` that `cat` carries to the log port; fd 3 holds a writer until sshd is done.
3. init makes /dev/virtio-ports from sysfs, at most 500 tries 10 ms apart. On timeout it prints
   `init: no virtio ports named io and log; powering off` to guest.log and powers off.
4. ssh exits first: written on the page ("ssh ends each guest").
5. `-no-reboot` and `panic=-1` in the proxy.
6. `-m 256M`, plus an omit list in guest.toml with one reason per entry (13 entries).
7. guest.log uses `append=on`; loopback() truncates it at case start, as it does sshd.log.
8. `/dev/console` (5,1) is written by the cpio writer; the merged-usr links are in guest.toml `links`.
Also: every directory goes into the archive as 0755 whatever the umask (sshd refuses a group-writable
privsep path). Each deb's `Version` field is checked against the recipe.

## Gates run (all inside in-dev)

| command | exit | note |
| --- | --- | --- |
| cargo testbench bench-ssh-loopback-openssh, 5 runs | 0 ×5 | PASS 89.9 s (first run: fetch + build), 3.5, 3.5, 3.7, 3.7 s |
| cargo test -p testbench | 0 | 88 passed, 4 of them new |
| cargo +nightly fmt --all --check | 0 | |
| cargo run -q -p redoubt-doccheck | 0 | after the pages commit |
| cargo testbench size-budget | 0 | PASS. testbench is not in the budget (no row) |
| git diff --check | 0 | |
| whole bench, rv64 and rv32, no --allow-skip | not run yet | waiting for the orchestrator's slot |

Not run: rv32 builds and the unsafe ratchet. This package touches no kernel, server or runtime code,
and testbench adds no `unsafe` (the keeper kills with the `kill` command, not libc::kill). The whole
bench's unsafe-budget case will cover the ratchet.

Image: tag 754f0d28b0085bc8, vmlinuz 31,165,952 B, initrd.img 17,824,808 B; debs cached in
target/ssh-reference/debs/ by sha256.

## Documentation check

- docs/testbench.md, "Sessions and the loopback server": the status lists the 4 new host tests
  (11 → 15). Line 715: "in a QEMU guest". The container paragraph and its bullets are replaced
  with the guest's: why, the image, each session, ssh ending it plus the keeper, and the probe with
  --allow-skip. Line 785 ("Nothing there needs ... a container") now says "a guest"; line ~795
  "Its server runs in a container" now says "a QEMU guest".
- GETTING-STARTED.md prerequisites: curl, dpkg-deb, xz, and the network once.
- Checked, no change needed: README.md (no mention); docs/plan/m1-separation.md:171 says
  "OpenSSH's server as the reference", with no container claim; docs/servers/sshd.md mentions
  OpenSSH only as the client.
- Grep for `podman|container|Containerfile` across docs/, GETTING-STARTED.md, Dockerfile, dev.sh
  and tools/. Remaining hits are other senses: boot.md and pkg.md (the signature container),
  keyd.md:266 ("no container"), the dev container in GETTING-STARTED.md:8,17,117, Dockerfile:8,
  dev.sh and tools/testbench run.rs, case.rs and build.rs, plus docs/theme/mermaid.min.js. In this
  section, testbench.md:723 explains why a container is not used, and :749 compares with the
  container.

## Open points

- The keeper kills a leftover QEMU after 5 s, in addition to failing the case. The brief said
  "fails and names it"; killing keeps a leak from outliving the bench.
- An HTTP(S)-proxy-only host: if a fetch fails there, the direct check finds the snapshot silent,
  and the failure counts as a host lack. This is the same rule as before, and the page says so.

---

# Final: gates on head (step 2 complete)

Base main 9212f6f60; head wp-BENCHENV1 f7a0cb1eb18310fa5b3c19f602a5eaf2154a445f (2 commits:
2f38e14d1 code, f7a0cb1eb pages). The tree was clean during the benches. I had the host alone,
with nothing else building.

| gate | command (all via .wash/local/in-dev) | exit | result |
| --- | --- | --- | --- |
| focused case ×5 | cargo testbench bench-ssh-loopback-openssh | 0 ×5 | PASS 89.9 s (built the image), 3.5, 3.5, 3.7, 3.7 s |
| whole bench rv64 | cargo testbench --arch rv64 | 0 | 226 PASS, 0 FAIL, 0 SKIP; reference case PASS 3.4 s |
| whole bench rv32 | cargo testbench --arch rv32 | 1 | 210 PASS, 1 FAIL, 0 SKIP; reference case PASS 3.4 s |
| host tests | cargo test -p testbench | 0 | 88 passed |
| format | cargo +nightly fmt --all --check | 0 | |
| docs | cargo run -q -p redoubt-doccheck | 0 | |
| size budget | cargo testbench size-budget | 0 | PASS (no testbench row) |
| whitespace | git diff --check | 0 | |

rv32's failure: `aio-many-reads-two [rv32, smp=1]` 21.1 s, first failing line
`forbidden output /aio-reader TEST FAILED/: [con 97f31e7290ff5a21] aio-reader TEST FAILED: receive: Timeout`.
- Rerun alone on head, 5 times: PASS, FAIL (same line), PASS, PASS, PASS. 10 more: 7 PASS,
  3 FAIL. That is 4 of 15 alone, on an idle host, so not only load.
- Same case on base 9212f6f60 (detached in this worktree, then back to the branch), 10 runs:
  9 PASS, 1 FAIL. It is intermittent before this package. The diff touches only
  tools/testbench/src/{ssh,ssh_guest,main}.rs, tests/ssh-reference/ and docs, and none of the AIO,
  kernel or QEMU boot path. I propose a follow-up against AIO1's case (an rv32 receive timeout)
  rather than a change here.

Logs: target/bench-rv64.log, target/bench-rv32.log (run dir
target/testbench/run-1-1791172566734860945 for the rv32 failure).

---

# Round 1 fixes (assignment 767d6b59)

The old head f7a0cb1eb is tagged `benchenv1-r1`. The new head is b65258b80, with two commits on
9212f6f60:
- cd36d49f5 (was 2f38e14d1): testbench: OpenSSH's reference server runs in a QEMU guest
- b65258b80 (was f7a0cb1eb): docs: OpenSSH's reference server runs in a QEMU guest

`git range-diff 9212f6f60 benchenv1-r1 HEAD` shows both commits `!` (changed) and none added or
dropped. Code: 3 files changed (ssh.rs, ssh_guest.rs, guest.toml). Pages: docs/testbench.md only.

## Fixes

- Red 1: `BUILDER_VERSION = 1` is hashed into `tag()`. Its comment says any change to what `build`
  or `Cpio` make of a recipe bumps it. The new tag differs from 754f0d28b0085bc8: the recipe
  changed too, and the image was rebuilt from cached debs.
- Red 2: `plain()` rejects ' '. `redoubt()` used to check its whole command line, spaces
  included; it now checks each word and joins them. "/w/a b" was added to the rejection list. All
  seven loopback cases pass, including Redoubt's.
- Red 3: the log relay is `while IFS= read -r line; do printf '%s\n' "$line"; done`, one write per
  line. The page now claims exactly that: a line of up to 4 KiB (PIPE_BUF, what sshd's write into
  the FIFO keeps whole) reaches the log whole. The init text is now a TOML literal string, so `\n`
  reaches the shell unchanged.
  Checked under load, 5 focused runs each time, inspecting every line of the case's sshd.log:
  - With a release-build loop (load around 3.7): 5/5 PASS in 4.4, 3.8, 3.5, 3.6 and 3.5 s; 318-325
    lines; 0 lines with a `debug1:` after the start.
  - With 24 sha256sum spinners plus the build loop (load 26-35 on 24 cores): 5/5 PASS in 6.3, 5.7,
    5.8, 4.9 and 5.9 s; 322-323 lines; 0 mid-line prefixes; every line's first word is one of
    sshd's own. The 2 "odd" first words in run 3 were whole lines: `Received disconnect from ...`
    and `Disconnected from user root ...`.
- Red 4: the probe's ssh has `ConnectTimeout=30` (`PROBE_TIMEOUT`), which bounds the wait for the
  banner; the page's probe sentence says so. The sessions are already bounded by the case's
  deadline (the driver gives up and kills ssh), so I did not change them.
- Red 5: init's comment now reads "at most 500 tries 10 ms apart".
- Simplifier 1: tag() uses userland::name; `hex` is gone.
- Simplifier 2: `verified` is inlined into `fetch`, which uses SOURCE. The test now drives
  `fetch` through a `file://` URL: a wrong sha256 is Broken and the file is not kept; the right one
  is kept under its sha256. `fetch_failed` keeps its source parameter for the Host/Broken test.
- Simplifier 5: the probe closure returns (output, case_dir).
- Editor 1: `cpio_padding_reaches_four` is folded into `the_cpio_writer_writes_newc`; the status
  stays at 15 names.
- Editor 2: "as it was from a container" is dropped.

## Gates

| command | exit |
| --- | --- |
| cargo test -p testbench | 0 (87 passed; one test folded) |
| cargo testbench bench-ssh-loopback-openssh ×10 under load (above) | 0 ×10 |
| cargo testbench bench-ssh-loopback (filter: all 7 loopback cases) | PASS ×7 |
| cargo +nightly fmt --all --check | 0 |
| cargo run -q -p redoubt-doccheck | 0 |
| git diff --check | 0 |

The whole bench was not rerun: no kernel or boot path changed.

---

# Final: gates on head b65258b80 (supersedes the earlier final section)

Base main 9212f6f60. Head wp-BENCHENV1 b65258b80: cd36d49f5 (code), b65258b80 (pages). The
reviewed round-1 head is tagged benchenv1-r1 (f7a0cb1eb). The tree was clean and the branch
unchanged during the benches, and I had the host alone.

| gate | command (all via .wash/local/in-dev) | exit | result |
| --- | --- | --- | --- |
| whole bench rv64 | cargo testbench --arch rv64 | 0 | 226 PASS, 0 FAIL, 0 SKIP; reference case PASS 3.6 s |
| whole bench rv32 | cargo testbench --arch rv32 | 0 | 211 PASS, 0 FAIL, 0 SKIP; reference case PASS 3.6 s (aio-many-reads-two passed this time) |
| focused case ×10 under load | cargo testbench bench-ssh-loopback-openssh | 0 ×10 | PASS 3.5-6.3 s, sshd.log lines whole |
| loopback cases | cargo testbench bench-ssh-loopback | 0 | 7/7 PASS |
| host tests | cargo test -p testbench | 0 | 87 passed |
| format | cargo +nightly fmt --all --check | 0 | |
| docs | cargo run -q -p redoubt-doccheck | 0 | |
| size budget | cargo testbench size-budget | 0 | PASS (testbench has no row; run on f7a0cb1eb, and no budgeted crate changed since) |
| whitespace | git diff --check | 0 | |

Logs: target/bench-rv64-r2.log and target/bench-rv32-r2.log.
