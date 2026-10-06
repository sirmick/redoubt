# The file servers parse their arguments and call their range three times over

## What

`littlefsd`, `erofsd` and `verityd` each parse `endpoint=NAME` and `labels=ID[,ID...]` with
their own copy of the same rules (`parse_args` and its `Args` and `BadArgs` in `littlefsd` and
`erofsd`, and `verityd`'s own), beside `buckets=N`, which the serving library parses once. Each
also has its own client of `blkd`'s protocol (`Blkd::call`, the reply's checks, the handles a
reply must not carry), and the 64-sector limit of one `read` is written in `blkd`, `verityd` and
`erofsd`.

## Why it matters

The rules are the manifest's ([init](../servers/init.md#the-boot-manifest)): a server that
misreads a label set serves under the wrong one, so three copies are three places one change
must reach, and three host tests of the same thing. The range client's checks are what keep a
reply of the wrong shape out of a parser; one copy is one place to get them right.

## Where

`servers/littlefsd/src/server.rs` and `servers/erofsd/src/server.rs` (`parse_args`),
`servers/verityd/src/lib.rs` (its arguments); `servers/littlefsd/src/blkd.rs`,
`servers/erofsd/src/blkd.rs` and `servers/verityd/src/blkd.rs` (the range clients);
`servers/blkd/src/virtio.rs`, `servers/verityd/src/server.rs` and `servers/erofsd/src/blkd.rs`
(`MAX_SECTORS`). The serving library's `buckets` and `own_args` are in `libs/rt/src/server`.

## Done when

- `endpoint=` and `labels=` are parsed in `redoubt_rt::server` beside `buckets=`, with one host
  test of the rules, and the three servers use it and drop their copies and their copied tests.
- One client of `blkd`'s protocol for a range, with `info` and `read` (and `write` and `flush` for
  `littlefsd`), and `blkd`'s read limit named once where the wire table is.
- `erofsd`'s residual that links this page goes, and this page is deleted.
