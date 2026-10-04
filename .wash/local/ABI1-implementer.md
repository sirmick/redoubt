# ABI1: the system-call ABI apart from its transport

Tier A (`libs/sys`, `libs/rt`'s one seam), size S. No needs: start from main. Run every cargo and
bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

The owner (2026-10-03): "Split the sys ABI from its transport. Call encode/decode is already pure,
but ecall.rs is the only way in. Put the transport behind a trait or feature so an in-process
cooperative backend plugs in under rt with nothing above it changing."

## Context rules (read these first)

- **Don't read whole files.** `libs/sys/src/ecall.rs` (34 lines) and `libs/rt/src/sys.rs` (63)
  whole; `libs/sys/src/lib.rs` only its module doc and the `mod`/`pub use` block; in
  `libs/rt/fake/src/lib.rs` only the install (search `install_host_kernel`) and the `impl`; in
  `servers/init/src/bin/init.rs` only the two `redoubt_sys::syscall` lines.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.**
- **Keep reports under 1900 bytes,** with detail in `.wash/local/ABI1-report.md`.

## Reading list (only these)

- `docs/kernel/abi.md`: "One register layout on both widths" (its last paragraph, on
  `redoubt_sys::syscall`) and "Why".
- `docs/userland/native.md`: the bullet "Tested on the host against a fake kernel".

## What exists

`redoubt_sys::syscall(&Call) -> Result<Return, Error>` encodes, runs `ecall` (the crate's one
`unsafe`) and decodes. `redoubt_rt::sys::syscall` is the runtime's one seam: on
`target_os = "none"` it calls `redoubt_sys::syscall`; on the host it calls the `HostKernel` a test
installed once (`OnceLock<&'static dyn HostKernel>`), which the fake kernel (`libs/rt/fake`) and
`scripted.rs` implement. The seam is chosen by the target, so a backend that is neither the
`ecall` nor the host's fake has nowhere to plug in.

## The settled design

1. **The trait lives in `redoubt_sys`,** beside the ABI it carries:
   ```rust
   /// Carries one call to a kernel and its result back. Records and buffers named by address in
   /// `call` stay valid (and, for results, writable) for the duration; the transport reads and
   /// writes them in place, as the kernel does. Never discard the result (R13).
   pub trait Transport: Sync {
       fn call(&self, call: &Call) -> Result<Return, Error>;
   }
   ```
   A call in, a `Return` out, records and buffers by address as today. Nothing else about a
   call changes: `Call`, `Return`, the records and `decode_result` are the ABI, the transport is
   how they travel.
2. **`Ecall`,** a unit struct in `ecall.rs` (riscv only, as now), implements `Transport` with
   today's body. `redoubt_sys::syscall` stays, as `Ecall.call(call)`, for what sits below the
   runtime by design (the stub, `stub/src/bin/fixture-child.rs`, `tests/programs`). The `unsafe`
   count does not move: the same one `asm!`.
3. **Selection: one installed transport, as the fake is today, with the `ecall` the default.**
   `redoubt_rt::sys::syscall` calls `Ecall` when built for `target_os = "none"` without the
   runtime's new feature `installed-transport`, and the installed `&'static dyn Transport`
   otherwise. `HostKernel` becomes `redoubt_sys::Transport` (re-exported by `redoubt_rt`), and
   `install_host_kernel` becomes `install_transport`; the fake and `scripted.rs` implement
   `Transport`. On the host the slot is today's `OnceLock`. A slot for a `no_std` target with
   `installed-transport` is the backend's own package (it must add no `unsafe` to `libs/rt`): until
   then that combination is a `compile_error!` naming it.
4. **Nothing above the runtime calls the `ecall` directly.** `init`'s two
   `redoubt_sys::syscall(&Call::DeviceInfo { .. })` calls go through the runtime (a thin
   `device_info` beside the runtime's device types, or the nearest existing one). The stub, its
   fixture and `tests/programs` stay on `redoubt_sys::syscall`: they test the kernel's ABI from
   below the runtime and run only on the machine. List every other direct caller you find.
5. **Unchanged:** every public type and function of `redoubt_rt` above `sys.rs` except the two
   renames, `redoubt-client`, every server, the ABI's encoding, the unsafe budget.

## The cases

1. **A host test in `libs/rt`**: a second transport wrapping the fake that counts and forwards
   each call sees every call a runtime operation makes (an echo call round trip), with no change
   above the seam.
2. **Every host test and every machine case unchanged:** `rt-host-tests`, the client's and the
   servers' host tests, and the whole bench on both widths.
3. **`unsafe-budget` and `size-budget`** pass with `libs/sys`'s count unchanged; report the rv32
   and rv64 image sizes of one server before and after (the `ecall` path must stay a direct call).

## Page lines (exact text in the report)

- **`libs/sys/src/lib.rs`, module doc:** after "What a call *does* is the kernel's business.", a
  paragraph: "How a call travels is a [`Transport`]: a call in, its [`Return`] out, records and
  buffers by address. On the machine it is [`Ecall`], the registers below; a host's fake kernel,
  or any other backend, implements the same trait and reads and writes the same records." The
  "# Registers" heading's first sentence names `Ecall`.
- **abi.md**, the paragraph "On the process's side, `redoubt_sys::syscall` is the `ecall` itself
  (the crate's only `unsafe`), and `redoubt_sys::decode_result` reads the result." becomes "On the
  process's side a call travels through a transport (`redoubt_sys::Transport`): on the machine
  `Ecall`, the `ecall` itself (the crate's only `unsafe`), whose result
  `redoubt_sys::decode_result` reads; the runtime takes whichever transport is installed, the
  `ecall` by default." The rest of that paragraph stays.
- **native.md**, "Tested on the host against a fake kernel": "on the machine it is the `ecall`, on
  the host a `HostKernel` a test installs" becomes "a `Transport`: on the machine the `ecall`, on
  the host the fake kernel a test installs, and any other backend the same way".
- **docs/beyond/README.md**, after the table, the owner's direction in one line: "**A direction,
  the owner's:** the system-call ABI is kept free of the MMU (a transport trait, lends and
  transfers stated as ownership), so that a backend without one, cooperative and in one process,
  could implement it. Nothing builds that backend."

## Owned paths

- `libs/sys/src/{lib.rs,ecall.rs}` (the trait, `Ecall`), `libs/rt/src/{sys.rs,lib.rs}`,
  `libs/rt/Cargo.toml` (the feature), `libs/rt/fake/src/{lib.rs,scripted.rs}` (the impl and
  install only), `libs/rt/tests/` (the new test, the renames), `init`'s two calls.
- The page lines above.

**Not yours:** the ABI's encoding, the kernel, the stub. **Hotspot:** ABI2 follows this package
and edits the fake's lend handling and `libs/sys` docs; it starts after this merges.

## Gates

- The whole bench on both widths, alone.
- `rt-host-tests`, the client library's host tests, every server's host tests, `libs/sys`'s.
- `cargo fmt --check`, the size and unsafe budgets, doccheck.

Report each command with its exit code, every direct caller of `redoubt_sys::syscall` and what
became of it, the sizes before and after, and each page line as written.
