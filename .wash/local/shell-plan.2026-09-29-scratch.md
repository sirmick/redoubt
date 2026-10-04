# Plan: the Redoubt shell (signed off by the owner, 2026-09-29)

Branch `wp-SHELL1`, worktree `.worktrees/SHELL1`. Not yet a node in `plan.toml`.

## 1. What the shell is

`Redoubt.Shell`: our own REPL in the session's beamlet VM. Elixir at the prompt; **not IEx**.
Built on OTP/Elixir public APIs; replaces only IEx's loop.

    /dev/cons (Redoubt) | raw tty (host)
      beamlet_io            VM-embedded byte pump (raw subscription)
      Redoubt.Shell.Driver  ours; replaces user_drv + prim_tty; draws only via Redoubt.Term
      group + edlin         OTP unchanged: editing, history, Ctrl+R, expand_fun
      Redoubt.Shell         ours; replaces IEx.Server

Loop: read (group/edlin) -> classify (first two tokens vs registry) -> command expands straight
to an AST (bare words = literal binaries) | else Code.string_to_quoted ("missing terminator" =
read more) -> eval in a throwaway evaluator (max_heap_size; :kill on interrupt; the shell holds
bindings + env, so earlier bindings survive) -> print as cells via Redoubt.Term (%Lines{} -> pager).

Gone: .iex.exs (shown: runs from the cwd with session authority), pry, remote shells, in-band
ANSI. Redoubt.Term is the only writer of control sequences. IEx remains only as the IexTest
difftest (VM conformance).

Public APIs relied on (checked present in 1.20.4): Code.string_to_quoted/2, Code.env_for_eval/1,
Code.eval_quoted_with_env/4, Code.Fragment.{cursor_context/2, surround_context/3,
container_cursor_to_quoted/2}. Avoid: IEx.Autocomplete/Evaluator/Introspection (all :hidden).
group's driver protocol: user_drv.erl:56-94 (requests), group.erl:507 (interrupt).

## 2. Source tree

    shell                  root dev entry (renames ./iex once it starts Redoubt.Shell)
    toolchains/            pinned OTP 28.5.0.6 + Elixir 1.20.4, gitignored, release layout
    userland/otp/          beamlet; adds raw tty + size/resize in CLI, raw subscription in beamlet_io
    userland/shell/        Mix, zero Hex deps; elixirc_paths includes ../../libs/wire/elixir
      lib/redoubt/shell.ex, shell/{driver,evaluator,printer,complete}.ex
      lib/redoubt/term/    ◆ encoder, screen+diff, key+paste decode, width
      lib/redoubt/util.ex, cmd/, sys.ex, sys/{host,native}.ex
    userland/native/       cargo workspace, excluded from root
      cells/ ◆  backend/  render/  ed/ (later)
    tools/beam-pack/       ◆ later: userland disk image + system.index

◆ = Tier A carve-out; the rest Tier B.

## 3. Host (dev aid until Redoubt is up)

Sys.Host is a strict subset of Redoubt: no /bin/sh, env, signals; files in the --root sandbox.
Run: ./shell; `mix test` (real BEAM); same suite on beamlet (runner like tools/elixir-tests);
`cargo test` in userland/native; render behind a port (--exec) in wave 2.
Works on host: editing, history, Ctrl+R, completion, loop + interrupt, helpers, command mode,
pager, help, hostile-text and command-injection attack cases.
Waits for Redoubt: ns/bind, labels, native pipes, jobs, resource use, consol parked resize
(host uses SIGWINCH), persistent history, vault behaviour.

## 4. Native libs (no_std + alloc; std only in host shims; rv64 build needs rustup)

cells ◆ (cell = pos, grapheme, fg, bg, attrs; no C0/C1/DEL by type) · backend (ratatui-core
Backend -> cells) · render (view tree in, changed cells out; two pipes + budget only) ·
ed later (ratatui-widgets, crop, regex, unicode-width/-segmentation; textarea + synoptic forks).

## 5. On Redoubt

| Store | Holds | Writable | Integrity |
| boot bundle | kernel, init, servers, beamlet (+8 embedded modules), system.index | no | signature (R15) |
| userland disk (new, RO virtio) | OTP, Elixir, Redoubt Elixir as one object per module named by hash; render, ed | no (readonly=on) | each object hashed vs system.index by its consumer |
| data disk | homes, /system/pkgs (M5), steward state, history | yes (littlefs/fsd) | packages signed; homes by capability |

Namespace: /boot (bootfsd) · /system/lib (userland disk) · /system/pkgs · /home/<p>.
load_module: name -> hash (index) -> fetch /system/lib/<hash> -> hash, compare -> loader.
Beams stripped + deterministic. Userland disk served by an fsd instance on the RO device.

Measured (stripped): IEx prompt closure 102 modules 1.03 MiB; core 7 apps 577 / 3.75 MiB;
+ crypto/ssl/ssh/etc 771 / 5.19 MiB (raw 28.16 MiB). bootfsd caps: 64 entries, 8 MiB.

## 6. Phases

1. Host, now, no M1 dep: G0 skeleton + ./shell + ExUnit on BEAM and beamlet · G1 raw console
   (restore tty on every exit) · G2 driver + Redoubt.Term (screen-model tests; hostile-text case)
   · G3 the loop (evaluator, interrupt semantics written down) · G4 helpers/Util/pager ·
   G5 command mode, defcommand, help, completion on Code.Fragment (injection case).
2. Host wave 2: cells/backend/render behind a port; pager + help on render; self-host stage 1
   (mix compile on beamlet).
3. Redoubt (needs M1 beamlet-on-Redoubt, init, fsd): Sys.Native; beam-pack; userland disk +
   system.index; shell on /dev/cons; attack cases as bench cases.
4. Rest of M2: bind/ns, native pipes + jobs, resource use, parked resize, persistent history, ed.
Later: on-box compiles (M4); stage 3 same-hash rebuild (DDC vs BEAM); signing (M5).

## 7. Decided

1. Shell = Redoubt.Shell, not IEx. 2. Tree as §2. 3. Host = dev aid, strict subset.
4. Control characters visible, absolutely; colour only as cell attributes from trusted code.
5. Separate RO userland disk pinned via system.index, checked per object by the consumer
   (extends verified boot to system userland; a new rule for the book).
6. Tier B for shell/native; ◆ carve-outs, platform boundary + natives, launch hash check,
   beam-pack, bench runner are Tier A.

## 8. Open

Owner: vendoring policy for userland/native · M2 nodes not needing M1 · rename ./iex -> ./shell.
During work: bracketed paste (none in OTP) · group.erl:918 hardcoded ESC in Ctrl+R prompt ·
full-screen interrupt key.

## 9. Book changes owed (via the Architect, with the packages)

shell.md (IEx -> Redoubt.Shell; line editing; Why) · beamlet.md (IEx on the console; load_module
row) · m1-separation.md, m2-usable-shell.md · packages.md (module sources) · bootfsd.md (scope) ·
new userland-disk integrity rule + SECURITY.md entry.

## Environment notes

- Worktree has no toolchains/: run with BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains.
- System rustc 1.93 builds beamlet and doccheck; not the riscv targets or nightly rustfmt.
- OTP source build at /tmp/otp_src_28.5.0.6 (useful for reading group/edlin/user_drv source).
