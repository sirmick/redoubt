# RCMD1 design: the shell's commands on Redoubt

Base: origin/main 44d970ab4. Worktree /home/mcloonan/redoubt/.worktrees/RCMD1, branch wp-RCMD1
(no code yet). Tier A: session authority, the steward red reviews.

## 1. What already works on the machine, by status lines and cases

| Piece | Built | On the machine |
| --- | --- | --- |
| `File` over 9P (prim_file → beamlet's platform → 9P) | built (files.md "Files over 9P") | `bench:beamlet-files`: an Erlang module under init, not a session and not the shell's commands. Write, read, list, remove, a parallel read, `eacces`, `enoent`, mode undefined. |
| A session's namespace (`/boot`, `/home/P`, `/vault`, `/net`, `/dev/cons`, `erofsd:system`) | built (sessions.md "Namespaces") | steward-ssh-two-principals and steward-vault-session show the connections made. No case walks a path from the shell. |
| `exec` | built | steward-vault-launch and beamlet-launch type `exec(...)` |
| `ns()`, `ns_lookup/1`, `bind/2` (commands, the `:redoubt` natives, the platform's `System::bind`) | built (shell.md "The shell in a session" says so; "Session commands" still says planned) | none: no case calls them |
| `Redoubt.Budget` carve and destroy, and `labels` (the native, ids) | built | steward-sub-budget-flood types `Redoubt.Budget.carve`; no case calls `:redoubt.labels` |
| The file commands (`ls`, `cat`, `cp`, `mv`, `rm`, `rm_rf`, `mkdir(_p)`, `touch`, `w`, `append`, `stat`, `find`, `glob`, `ls_r`, `cd`, `pwd`, `hexdump`, `checksum`, the line commands) | built, host only (shell.md "Files and text": "runs on the host only, over files beamlet's host platform serves") | none. `grep` over tests/*.toml finds no case typing any of them. |

Every machine case types only expressions, `exec` or `Redoubt.Budget`. Nothing the M2 page calls
"the commands on Redoubt" has run in a session.

## 2. What is missing or broken on Redoubt (found by reading; to confirm with a probe case first)

1. **`File.write_stat` always fails on Redoubt, and with it `File.cp`, `File.cp_r` and
   `File.touch`, so the shell's `cp`, `touch` and a cross-volume `mv` fail.**
   - OTP's `prim_file:write_file_info` always calls `set_owner(Uid, Gid)`, which becomes
     `set_owner_nif(-1, -1)` when both are undefined, as they always are on Redoubt. beamlet maps
     that native to `not_supported`, so the call returns `enotsup` before anything else happens.
   - It also always calls `set_time`, which Redoubt's platform refuses (`enotsup`, platform.rs
     default).
   - Elixir 1.20's `File.cp` ends in `copy_file_mode`, which calls `write_stat(dest, dest's own
     info with src's mode)`. So the copy is made and then the call fails. `File.touch` is
     `change_time`.
   - The host passes, because the host platform sets times and owners. files.md's "the mode
     preservation in `File.cp` ... is adjusted in beamlet's platform layer to skip the mode" is
     not built: the mode is skipped, but owner and time are not.

   Proposed fix, in beamlet's platform: a change that changes nothing succeeds.
   - `set_owner_nif(P, -1, -1)` is POSIX's "leave both as they are". It is `ok` after a check
     that P exists (a walk), and `enotsup` for any real id.
   - `set_time(P, A, M, C)` is `ok` when the times equal what the platform's stat reports for P
     (today 0 for all three, the residual), and `enotsup` otherwise.

   So `File.cp`, `File.cp_r` and Mix's copies work unchanged, and nothing is faked: a real
   change of time or owner is still refused visibly. `touch` of a new file then works (create,
   then times equal to the stat's). `touch` of an existing file asks for now, and stays
   `enotsup`. The shell's `touch` page says so ("a file with no stored time is not touched").

   Alternative: the shell's `cp` and `mv` copy without `File.cp`. That fixes only the shell;
   Mix and user code keep failing.
2. **Within one volume, `cp` is a read and write loop**, not the file server's `copy_file`.
   files.md's table says it is the server's. littlefsd and walfsd serve `copy_file` (typed,
   host-tested), and libs/client has it; the platform has no native, and `Redoubt.File`
   (`copy_file`, `rename`, `set_attr`, `get_attr`, named in files.md "How to use it") does not
   exist.
   - Proposed: one platform operation `copy_on(src, dst)`, beside `rename_on`. It makes a typed
     call on the VM's thread when both ends are on one connection, and gives `exdev` otherwise.
   - It is exposed as `Redoubt.File.copy_file/2`, and the shell's `cp` uses it first, falling back
     to `File.cp` on `exdev`.
   - Question A: `set_attr` and `get_attr` in this package, or left planned? I recommend left
     planned: no command needs them yet.
3. **`whoami()` and `labels()` have nothing to read.** The steward hands a session
   `budget_pages=`, `endpoint=` and the shell, and no principal name. `:redoubt.labels()` gives
   label ids, and the names are only in the steward's manifest lines.
   - Proposed: the steward adds two launch arguments it already knows, `principal=NAME` and
     `label=NAME:ID`, one per label of the session's set.
   - They are information, not authority: a session that rewrote them would lie only to itself;
     what it can reach is still its badges.
   - `whoami()` returns the name; `labels()` returns `[%{name, id}]`, checked against
     `:redoubt.labels()`, so a mismatch is visible.
   - This touches the steward's launch, `servers/steward/src/bin/steward.rs`, which is steward
     red territory.
4. **`follow(path)` "until Ctrl+C" cannot be built yet.** The driver drops the interrupt while a
   line is evaluated ("ending that is not the driver's yet"); that is M2 step 4 (Jobs).
   - Question B: defer `follow` to the Jobs package (my recommendation), or ship
     `follow(path, seconds)` now?
   - Either way it polls by `stat` size, since 9P has no notification.
5. **`clear()`** is the driver's existing `:clear` drawing (`Redoubt.Term`, CSI H, CSI 2J), asked
   for by a command through group. It is host-testable, and part of the drawing path, so strict,
   but it needs no machine case.
6. **`now()`, `today()`, `ago()`** wait for M6 (a session has no wall clock). They stay planned,
   and the page already says so.
7. **The page's status lines.** "Session commands" says planned, but `ns`, `ns_lookup` and
   `bind` are built. "Files and text" and "Copying, moving, removing and binds" go from host-only
   or planned to built and tested on the machine by the new cases. files.md's
   adjusted-in-the-platform sentence becomes true with fix 1.

## 3. Binds' authority model (what the page and the red should hold the code to)

- **A bind is a VM-local table edit.** `bind(prefix, conn)` adds `prefix → conn` to the platform's
  `Namespace`. `conn` must be a handle object the VM already holds: a connection, or an endpoint
  the VM was handed, which the platform attaches once over 9P. A budget is refused
  (`not_a_connection`). No kernel call grants anything, no server is told of the bind beyond
  that one attach, and no other session sees it. One connection under two prefixes has one badge
  and one set of labels, so every server check is unchanged.
- **Attaching a named non-9P endpoint** (`bind("/s", steward)`) sends a `Tattach` to a server
  the session can already call with `:redoubt.call`. It gains nothing, and a refused attach is
  `not_a_connection`, within `ATTACH_US`. To confirm in review: the steward answers a 9P attach on
  a session badge with a refusal, not a fault (a host test).
- **Replacing a prefix.** A bind at an existing prefix replaces it (`Namespace::bind` retains
  everything but that prefix). That includes `/dev/cons` and `/boot`, which `exec` then uses:
  `exec` reads its program from whatever `/boot` names and gives the child a fresh connection
  minted at whatever `/dev/cons` names. Both are within the session's own authority (a session
  can run any code it writes; native.md, "No signature is needed to run code within one's own
  authority"). The page states it, so no one reads `/boot` in a session as "the boot bundle".
- **Children.** A child gets only what `exec` writes: today exactly `/dev/cons`. The session's
  binds never reach a child unless a later launch passes them (native.md).
- **Bounded.** Today the table grows by one entry per new prefix, with no cap, in the platform's
  heap, which is the session's own. Proposed: a cap (64 entries, an existing wire error name,
  e.g. `too_large`), so a bind loop fails visibly instead of aborting the VM on allocation.
  Question C: the name and the cap.
- **Cleaned paths only.** A prefix must be clean and absolute (`bad_name` otherwise), and a
  lookup cleans its path, so `..` never climbs out of a bind.
- **No unbind.** None is proposed: a session ends with its binds, and nothing persists. A
  `bind(prefix, nil)` to remove one is a later question for M6's saved namespaces.

## 4. Cases, on rv64 and rv32 (steward and sshd, as steward-vault-launch)

Before any code, a probe: the case below, run on main as it is, to confirm what fails (fix 1 is
predicted). Its expected lines are then the package's verdicts.

`shell-commands` (one case, two sessions over SSH, verdicts from the shell's printed values and
the servers' lines, rule F):
- **alice's plain session:**
  - In her home: `mkdir_p("/home/alice/rc/d")`, `w(["one","two"], ...)`, `cat(...) |> count()`,
    `ls`, `cp` within the volume (the server's `copy_file`: the probe and walfsd's counted bytes),
    and `mv` within the volume (a rename).
  - `cp("/boot/beamlet-hello", "/home/alice/rc/")`, across volumes, read-only source. Then
    `checksum` of both, which must be equal.
  - `stat(...).size`, `touch` of a new file, `ls_r`, `find`, `glob`, `cd` and `pwd`, a
    relative path, `rm`, and `rm_rf` of what it made (it cleans up, for a replay).
- **binds in the plain session:**
  - `{h, _} = ns_lookup("/home/alice"); bind("/h", h)`, then `ls("/h/rc") == ls("/home/alice/rc")`.
  - `ns()` lists `/h`.
  - `cat("/home/bob/x")` is enoent, and `bind("/b", budget)` is `not_a_connection`.
  - `bind("rel", h)` is `bad_name`, and 65 binds hit the cap (question C).
- **whoami and labels:** `whoami()` is `"alice"` and `labels()` is `[]`.
- **alice's vault session (alice+alice-secrets):**
  - `whoami()` and `labels()` give the label's name and id.
  - `cp("/home/alice/rc/x", "/vault/x")` is a cross-volume read up, and works.
  - `w([...], "/home/alice/rc/y")` is refused, a write down: the error name, from walfsd's label
    check.
  - `mv("/vault/x", "/vault/z")` is a rename in the vault.
  - `mv("/vault/z", "/home/alice/")` is refused at the copy's write, and the source stays.

  This is a case with an attack in it: the vault session is the attacker trying to write down.
  The verdict is walfsd's refusal and the file's absence, read by the plain session afterwards,
  not anything the vault session claims.
- **What is not shown on the image:** a successful cross-volume `mv`. No session holds two
  volumes it may both write and remove on. It stays host-tested (the shell's suite); the page
  says so.

Host tests:
- beamlet: `set_owner(-1,-1)` and `set_time` equal to the stat succeed, and any other value is
  `enotsup` (beamlet-redoubt on the fake kernel with littlefsd); `copy_on` gives `exdev` across
  connections.
- The bind cap.
- The steward: launch arguments for principal and labels, and a 9P attach on a session badge.
- The shell's `whoami`, `labels`, `clear` and `cp`'s order.

Gates:
- `./test-shell`;
- the shell set on both widths, plus `shell-commands`, steward-vault-launch and
  steward-ssh-two-principals;
- beamlet-files;
- beamlet-footprint on both widths;
- doccheck, formatting, size-budget and the unsafe count.

## 5. Pages

- docs/userland/shell.md:
  - "Session commands": the status line; `whoami` and `labels`; `follow` as decided in question B;
    the clock stays M6.
  - "Files and text": the status line on the machine; `touch` on an existing file.
- docs/userland/files.md:
  - "Copying, moving, removing and binds": the status line, and the bind rules above (replacing,
    the cap, `/boot` and `/dev/cons` under a bind).
  - "Files over 9P": the sentence on owner and time "no change".
- docs/userland/beamlet.md: platform owner and time.
- docs/servers/steward.md: the session's launch arguments.
- docs/plan/m2-usable-shell.md: step 2's state.

## 6. Proposed commits (each with its tests and pages)

1. beamlet: owner and time "no change" succeed; `copy_on` and its native; the bind cap.
2. steward: a session's principal and label names as launch arguments.
3. shell: `Redoubt.File.copy_file`; `cp` within a volume through it; `whoami`, `labels` and
   `clear`; the pages.
4. tests: `shell-commands` on both widths.

## Questions for you

- A. `set_attr` and `get_attr`, with `Redoubt.File`: in RCMD1, or left planned? I recommend
  planned.
- B. `follow`: defer it to Jobs (step 4), or ship it with a time limit now? I recommend deferring.
- C. The bind cap: 64 entries, with which error name (`too_large`?), or no cap (it is the
  session's own heap)?
- D. Fix 1 in the platform (no-change owner and time succeed, which makes Elixir's `File.cp` and
  `File.touch` work for every caller), rather than in the shell's commands: agreed?
- E. The steward's launch arguments for `whoami` and `labels`: acceptable, or do you want a
  typed `whoami` call on the steward instead (more surface, live data)?

## Probe results (rv64, alice's plain session; tests/shell-commands.toml, uncommitted)

1. **Blocker: a session cannot write in its home.**
   - `File.mkdir("/home/alice/rc")` gives `enospc`, and `mkdir_p`, `w`, `cp` and `mv` all fail
     behind it. Reads work: `ls` shows `hello`, and stat of the binding's root is a directory.
   - Cause: the steward mints the home connection with quota 0 (steward.rs:190,
     `new_connection(&mut self.lend, root, 0)`), and walfsd lets a root with quota 0 read and
     remove only. The vault slot is minted with 0 too.
   - Proposal: a per-principal byte quota in the manifest. `home` gets
     `{"at": "data:/home/alice", "quota": "8388608"}`, and each label set with a vault gets
     `"quota"`. init's check refuses a volume whose homes' quotas exceed its room. The steward
     line becomes `home ... quota=N`, and the vault's likewise, and the steward passes it to
     `new_connection`.
   - Image values: data is about 32 MiB, so alice 8 MiB and bob 8 MiB; the vault is about 32 MiB,
     so alice-secrets 8 MiB.
   - It touches the manifest, init's check, the core's line parser and writer
     (libs/steward/src/manifest.rs) and the image manifests. Tier A.
2. **`File.touch` of a path in a missing directory returns `:ok` and creates nothing.** OTP's
   `prim_file` ignores the `enotsup` that `set_owner` and `set_time` return, and the natives do
   not check that the path exists. Fix in beamlet: `set_owner` and `set_time` walk the path
   first, giving `enoent` if it is missing, and only then `enotsup`.
3. **Works:**
   - `ls` of a binding, `stat`, `File.dir?`;
   - `ns()`, `ns_lookup`, and `bind("/h", ...)`, which returns ok (its contents untested, behind
     blocker 1);
   - `/home/bob/x` is enoent, a bind of a budget is `not_a_connection`, and a relative prefix is
     `bad_name`;
   - `:redoubt.labels()` is `[]`, and `rm_rf` works.
