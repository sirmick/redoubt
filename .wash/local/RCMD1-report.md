# RCMD1 report: commits (a) to (c), for review; (d) waits for HOME1

Branch `wp-RCMD1` (worktree /home/mcloonan/redoubt/.worktrees/RCMD1), on origin/main 8fa39c5ad.
Head 0a71844d6. Not pushed. The design and the probe results are in
.wash/local/RCMD1-design.md.

## Commits

1. **3f6d67f7e (a): beamlet.** A field set on a missing file is enoent; the server copies within
   one volume; binds are capped.
   - The Files trait gets the default `unsettable`: `set_times`, `set_permissions` and the new
     `set_owner` look the path up first, so a missing path is enoent and a path that exists is
     enotsup. `set_owner_nif` is now `file::set_owner`, no longer `not_supported`.
   - Why: OTP's write_file_info takes enotsup as done, so before this File.touch of a file in a
     missing directory returned ok and made nothing (seen on the machine).
   - `redoubt:copy_file/2`: `Files::copy_file`, which on the Redoubt platform is `copy_on`, the
     server's typed copy_file made on the VM's thread, as `rename_on` is. It gives exdev across
     connections, eexist onto an existing file, and eacces onto the namespace's own directory.
   - `MAX_BINDINGS` is 64: a new prefix past it is `system_limit`, checked before any attach; a
     bound prefix is still replaced.
   - files.md is corrected. `File.chmod`, `File.chown`, `File.write_stat` and `File.touch` on a
     file that exists return :ok and change nothing, by OTP's contract (confirmed on the machine:
     File.chmod is :ok); a missing path gives enoent. The page no longer says the platform
     skips File.cp's mode.
   - beamlet_files.erl and bench:beamlet-files gain two lines: a file's info written back is ok,
     and info written to a missing file is {error,enoent}.
2. **a0de86d06 (a2): beamlet.** `redoubt:identity/0` gives
   `{ok, #{principal, labels => [{Name, Id}], context}}`, read from the launch arguments
   `principal=`, `label=NAME:ID` and `context=`; a VM with no principal= gets not_found.
   - beamlet's binary does not take these arguments for the module to run.
   - New section in sessions.md, "What a session is told".
3. **cd1392c96 (b): steward.** `own::session_args` writes the three arguments.
   - `Kernel::launch` takes the session's context, which drive reads from the core's session
     record (owner kind Session only).
   - The size-budget ceiling for servers/steward goes from 1212 to 1224, with its reason line in
     the commit message.
   - steward.md, "Authentication and sessions", is updated.
4. **0a71844d6 (c): shell.**
   - `whoami()` gives the principal, plus ".context" for a named context; `labels()` gives the
     kernel's label ids by the steward's names. On a host they are nil and [].
   - `clear()` goes through `Redoubt.Shell.Driver.of_group/0` and a `{:redoubt_clear, pid, ref}`
     clause in the driver. `Redoubt.Screen` now finds its driver through `of_group` too.
   - `Redoubt.File.copy_file/2`; `cp` tries it first and falls back to `File.cp!`.
   - Pages: shell.md "Session commands" is now built (follow waits for the interrupt to end a
     line, the clock for M6), with the cp row; files.md "Copying, moving, removing and binds" is
     now built, partly tested, with the binds' authority points (what replacing /boot and
     /dev/cons does, a child gets /dev/cons alone, one connection has one badge).

## Gates on 0a71844d6 (8fa39c5ad base)

- `q run --cores 8 -- ./test-shell`: exit 0, every stage passed.
- `make -f scripts/jobs.mk prebuilt`: 0 failed.
- `make -k -j -f scripts/jobs.mk set CASES="steward-* sshd-* $(scripts/shell-cases origin/main)
  no-cruft unsafe-budget size-budget formatting elixir-oracles"`: exit 0, 83 PASS. That includes
  beamlet-footprint on rv64 (7.6 s) and rv32, beamlet-files, size-budget and unsafe-budget.
- `tools/difftest` (on 24241adcc; the beamlet code is the same after the rebase): 527/527 passed,
  21 skipped by design.
- Host tests:
  - beamlet-redoubt (fake): files 21, system 15;
  - beamlet-vm: system 16, the rest green;
  - redoubt-steward-server: all green;
  - beamlet-redoubt built for riscv64 and riscv32; the steward binary built for both widths.
- doccheck exit 0. rustfmt +nightly and mix format clean.

## In a real session (probe, rv64; not a committed case)

- whoami() is "alice"; labels() is [] in the plain session and ["alice-secrets"] in the vault.
- :redoubt.identity() is `{:ok, %{context: nil, labels: [], principal: "alice"}}`.
- copy_file from /boot to the home is exdev; File.chmod is :ok.
- ns(), ns_lookup and bind work, with their bad_name and not_a_connection refusals, and
  /home/bob is enoent.
- Writing in a home waits for HOME1. The vault was writable already; ruled by design.

## Summaries checked

- Updated: docs/userland/files.md, shell.md, sessions.md, beamlet.md, docs/servers/steward.md.
- Checked, no change needed:
  - docs/plan/m2-usable-shell.md: step 2 is done only with (d).
  - GETTING-STARTED.md: no session commands are named.
  - README.md.

## Open / next

- (d): the shell-commands case, plain and vault sessions on both widths, stacked on HOME1
  (wp-HOME1 6934fed86), on a separate branch until HOME1 is on main. With it come shell.md's
  "machine-tested" statuses and M2 step 2's state.
- Risk: copy_on blocks the VM's thread for a server copy, as rename_on does; it is bounded by
  the 8 MiB home quota.
