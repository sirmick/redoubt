# The littlefs hostile images are not in the repository

## What

Three of littlefs's hostile-input tests read hand-built volume images with `include_bytes!`:
`images/dup-names.img`, `images/nul-names.img` and `images/oversized-inline.img`. The
repository's `.gitignore` ignores every `*.img`, and none of the three is tracked, so a fresh
clone cannot compile littlefs's hostile tests at all; they build only in a checkout that still
holds the files from when they were made.

## Why it matters

These tests attack [R49 (a hostile medium is corrupt, not a crash)](../servers/fsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash):
duplicate names, NUL bytes in names and an oversized inline file from another writer. A test
that only one machine can build does not hold the rule for anyone else, and a reviewer cannot
see the bytes it runs on.

Belongs to no follow-up package: a test-data fix, taken with the next change to littlefs or
its tests.

## Where

- [`libs/littlefs/tests/hostile.rs`](../../libs/littlefs/tests/hostile.rs): the three
  `include_bytes!` tests.
- [`.gitignore`](../../.gitignore): the `*.img` rule.

## Done when

- A fresh clone builds and passes littlefs's hostile tests: the three images are tracked (with a
  negated ignore rule for `libs/littlefs/tests/images/`), or the tests build the images
  themselves.
