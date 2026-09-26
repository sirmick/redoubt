# Loader stub test coverage

## What

The loader stub parses the one input in a launch an attacker can shape, the ELF image, and parts of
what it checks are not attacked on their own:

- Its fuzz target (`stub/fuzz/fuzz_targets/plan.rs`) builds, but has never been run as a fuzz
  campaign.
- On target, dropping the stub's segment-versus-segment or startup-page overlap check still ends
  in the same refusal (exit 111), because the kernel's `map_fixed`, which never replaces a mapping,
  refuses the segment anyway. The bench cannot tell "the stub checks it" from "the kernel refuses
  it"; only host tests pin the stub's own checks.
- No bench case produces exit 112 (a segment `map_fixed` cannot fit in the child's budget): a
  regression that answered 111 instead would pass.
- Nothing observes that the stub unmaps the image copy before it jumps; removing the unmap passes
  every test.
- `tests/programs/build.rs` rebuilds the bench's stub when `libs/sys/src` or `libs/wire/src`
  changes, but not when their `Cargo.toml` or the workspace `Cargo.lock` does.

## Why it matters

A hole in the stub's checks is a hole in [R32 (a hostile image hurts only its process)](../servers/init.md#r32-a-hostile-image-hurts-only-its-process)
that every other test hides: the kernel's refusal stands in for the stub's on target, and an
unfuzzed parser has only been attacked with the cases people thought of.

Fixed in the servers follow-up package after the documentation rewrite.

## Where

- [`stub/src/lib.rs`](../../stub/src/lib.rs): `plan`, `image_in_bounds`, `read_image`.
- [`stub/src/main.rs`](../../stub/src/main.rs): the exit codes and the unmap.
- [`stub/fuzz/`](../../stub/fuzz): the fuzz target.
- [`tests/stub-launch.toml`](../../tests/stub-launch.toml) and
  [`tests/programs/build.rs`](../../tests/programs/build.rs).
- The page: [init](../servers/init.md#residual-risks).

## Done when

- The fuzz target has run as a campaign for a stated time, with its corpus kept, and a bench case
  or scheduled job runs it again.
- A host test pins each overlap check the kernel would mask on target.
- A bench case with a segment too big for the child's budget expects exit 112.
- A test observes that the image copy is unmapped when the program starts.
- The bench's stub is rebuilt when `libs/sys` or `libs/wire` manifests or the lock file change.
