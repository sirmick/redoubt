# STEWARD2 handoff (steward2-implementer-2 -> next), 2026-10-06

## Branch state FIRST
- wp-STEWARD2 HEAD 414c606a5 (WIP), 2 WIP commits on 03e64723c: b1b8de725 (sshd box, ended, bootfsd budget, expect_after, vault, cases) and 414c606a5 (pages, steward-restart, sshd diagnostics, vault fsd labels). Tree clean. Nothing pushed.
- tests/steward-login-refused.toml carries a TEMPORARY `ssh_args = ["-vvv"]` on the alice session (debug); remove before commit.
- Env file /tmp/s2env.sh (jobserver env, PATH incl /home/mcloonan/redoubt/.wash/local and ~/.cargo/bin, RUSTSBI_PROTOTYPER(_RV32), BEAMLET_TOOLCHAINS, MK=make -f .wash/local/jobs.mk -C <wt> with those vars on the command line).
- TRAPS: never pkill with a pattern in your own command (killed my run); never python `open(p,'w').write(open(p).read())` (truncated a file).

## THE BLOCKER now
Every SSH case: sshd listens, init/steward fine, alice's console session works, sshd logs "connection 1 on slot 0", "connection 2 on slot 1", then NOTHING (no "a connection failed", "closed", "input ended", "could not be sent").
- -vvv result (rv64 steward-login-refused, run-223020-1791290977076322223): TCP connects ("Connection established"), ssh sends its version, then "kex_exchange_identification: Connection closed by remote host": the guest side closes before sshd's ident arrives. Look at the driver's first steps after ACCEPT in servers/sshd/src/bin/sshd.rs connection()/drive(): the three fid opens (ctl, data for the reader, data again for writes: ipd may close/refuse a second open of data), the reader's first read, the first ipd write of the server ident (Write::Wait?), and whether close_socket runs. Also suspect: driver stack 16 pages with no guard (sunset moves ~4.6 KiB state by value). Two connections appear per ssh (connection 1 may be the bench's probe).
- sshd now retries listen each second (no-net boots had sshd exit 3 x5 -> reboot).

## Cases status (rv64): none pass yet. steward-restart ran before the programs fix: the steward died (heap 10) and restarts said "users not empty", but the reboot was sshd's (now fixed by the listen retry); rerun. init-boot/userland-*/verity-* not rerun since the image gained sshd/vault/homes (init-boot counts set to 13 servers, 14 badges, 11 consoles, from a case-2 boot).

## Remaining, in order
1. Fix the sshd connection; pass cases 2-6 + restart rv64 then rv32; init-boot, userland-boot/read-only/bad-start, verity-* both widths.
2. Measure peaks (memory=true scans): sshd, ipd (heap 64 provisional), steward, fsd:alice-secrets; declare 2x; docs/testbench.md memory table (drop beamlet row; add steward, sshd, fsd:alice-secrets; ipd/fsd:system/bootfsd new values).
3. Keep sshd's operational lines (connection N on slot S, failures); drop any that are pure debug.
4. Fold into logical commits (handoff 1's list + bootfsd own commit "bootfsd: ...", testbench own commit "testbench: a case can expect console lines its sessions cause", Unsafe budget: sshd line, Size budget lines; commit 9 into 7; WIP image commit becomes the image commit).
5. Gates: doccheck (cargo test -p redoubt-doccheck --test docs), wire-gen/steward-gen --check, nightly fmt, unsafe-budget, size, rv32 builds; the whole bench is the train's.
6. Report .wash/local/STEWARD2-report.md: owned-path extensions (servers/bootfsd + bootfsd.md; tools/testbench/src/{case,qemu}.rs + testbench.md SSH sessions; libs/client launch.rs); ended touched consoled, libs/rt ninep_mux Around::send, libs/client console.rs; case 5 session-badge login host-only; steward-restart cannot show the console session back (the heap provocation repeats after reboot) -> tell orchestrator.

## Rulings in force
Brief dated sections incl. "how sshd learns a session ended" (a) and "The restart, corrected" (a); orchestrator: bootfsd 1 MiB (a); expect_after (a); case 5 third part host-only.

## What consumed my context
Bench waits (pool blocked by a solo run), case-file errors, the sshd box platform and 12 pages.
