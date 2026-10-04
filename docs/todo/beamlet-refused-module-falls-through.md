# A module the userland check refuses would be looked for on the code path

## What

`locate_module` in `userland/otp/vm/src/vm.rs` asks the platform's `load_module` first and, on
`None`, looks in the code path's directories. On Redoubt, `load_module` answers `None` both for a
module `system.index` lacks and for one whose object fails the check
([R75 (verified userland)](../kernel/boot.md#r75-verified-userland)), so a refused system module
falls through to the code path.

## Why it matters

R75 says a refused lookup finds nothing, with nothing looked for elsewhere. Today that holds only
because beamlet on Redoubt has no file system (`files` is the platform's default, none), so the
code path finds nothing. Once files exist (BEAM3) and a session can put a directory on its code
path, a module whose object was tampered with could load from that directory instead, by name.

## Where

`userland/otp/vm/src/vm.rs`, `locate_module`; `userland/otp/vm/src/platform.rs`, the
`load_module` and `load_app` methods, which return `Option`; `userland/otp/redoubt/src/lib.rs`,
whose module source knows the difference (`Unloaded::Absent` against `Unloaded::Refused`).

## Done when

The platform's lookup answers three ways, found, absent and refused, and `locate_module` stops at
a refusal; a host test in `beamlet-vm` shows a refused module is not loaded from a code path
directory holding it. Before files reach the platform (BEAM3) or launching does (BEAM4).
