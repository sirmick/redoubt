# The code path searches a session's directories before the system's

## What

beamlet looks a module name up in code path order: first the directories added to the front with
`code:add_patha/1`, then the platform's modules (`Platform::load_module`), then the directories
added at the end ([`userland/otp/vm/src/vm.rs`](../../userland/otp/vm/src/vm.rs),
`locate_module`). That is BEAM's order, and no module is sticky. So a directory a session adds to
the front of its code path, in its own writable files, shadows a module of the system bundle.

The ruled rule for installed code is the other way round: the system bundle always resolves first,
a package may not define a module the bundle defines, and implicit code-path loading never finds a
module in the session's writable namespace ahead of the bundle's
([packages](../userland/packages.md#profiles-and-upgrades)). beamlet departs from it.

## Why it matters

Shadowing reaches only the VM that set its own code path, so it grants no authority the session
did not hold: a session can already load any bytes with `code:load_binary/3`. What it breaks is
predictability. A `.beam` planted in a person's home, once that directory is on the front of the
code path, replaces a system module (`File`, `Redoubt.Shell`) the next time the name is looked up,
and the person sees system behaviour change without having loaded anything on purpose.

Fixed in the beamlet follow-up package after the documentation rewrite.

## Where

- [`userland/otp/vm/src/vm.rs`](../../userland/otp/vm/src/vm.rs): `locate_module` and
  `find_in_code_path` (the front directories searched before the platform).
- [`userland/otp/vm/src/bif/code.rs`](../../userland/otp/vm/src/bif/code.rs): `code:add_patha/1`.
- [`userland/otp/vm/src/platform.rs`](../../userland/otp/vm/src/platform.rs): the comment on
  `load_module` calling it the only way code enters, which `code:load_binary/3` contradicts.
- The pages: [beamlet](../userland/beamlet.md#the-platform-boundary) and
  [packages](../userland/packages.md#profiles-and-upgrades).

## Done when

- A module the system bundle defines resolves from the bundle, whatever the code path holds; a
  lookup never finds it in a directory added with `code:add_patha/1` first.
- A host test: a VM adds a directory holding a `.beam` for a system module's name to the front of
  its code path, and the lookup still returns the bundle's module.
- The `load_module` comment in `platform.rs` says it is one step of the lookup.
