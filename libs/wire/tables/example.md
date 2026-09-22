# Example protocol (codec conformance fixture)

Not a real protocol: no server speaks it. It exists so the generated codecs, the test vectors
(`libs/wire/vectors/`) and the fuzz targets exercise every field type, both message shapes,
replies, error replies and the file framing. Real tables live in the owning server's note
under `docs/`; each server package writes its own. `docs/WIRE.md` is the specification;
this note is the practical guide to writing one, and the generator enforces every rule here.

## Writing a table
A protocol is two tables: its messages and its errors.

**The message table.** Put a line `<!-- wire: NAME -->` directly above it (blank lines between
are allowed), where NAME is the protocol's name: snake_case, unique across all notes, and it
becomes the Rust module `redoubt_wire::proto::NAME` and the Elixir module
`Redoubt.Wire.Proto.Name`. The header must be exactly:

```
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
```

Then one row per message:
- **Opcode:** decimal, 1 to 4294967295, no leading zeros, unique in the table. 0 is reserved:
  word 0 of a reply is a status, and 0 means success.
- **Message:** the name in backticks, snake_case (`[a-z][a-z0-9_]*`). It becomes a Rust type in
  CamelCase (`read_block` is `ReadBlock`, its reply `ReadBlockReply`), so names whose type the
  generated code already uses (`message`, `reply`, `error`, `result`, `ok`, ...) are refused,
  as are Rust and Elixir keywords.
- **Fields:** the request's fields in order, each `` `name: type` ``, separated by commas, or
  `-` for none. Types: `u8`, `u16`, `u32`, `u64`, `string` (`u16` length + UTF-8), `bytes`
  (`u32` length + bytes), and `handle[N] KIND` (one ASCII space between them): the handle in
  slot N, which must name an object of kind KIND, one of `endpoint`, `budget`, `process`,
  `mmio`, `irq` or `reset` (KERNEL-SPEC.md, Objects). Slots are numbered 0, 1, ... in order,
  at most 4 (`MAX_MSG_HANDLES`), and carry no bytes. The kind is required, and an unknown one
  is refused, but it is documentation: the generator puts it in the generated codecs' docs
  only, and nothing checks it on receipt, since the kernel does not report a received
  handle's kind. A handle of the wrong kind is found by use: `WrongObject` on its first use,
  and the server replies `malformed`. There are no compound types; write a label set or an
  address as `bytes` and state its inner layout in the note.
- **Reply:** the reply's fields in the same form as Fields (with its own handle slots from 0),
  or `-` for a reply that is its status alone.

There is no column saying how a message is sent: every milestone 1 typed message is a `call`
(WIRE.md, answer 98), and a table with any other header is refused. A `kind` column is added
when a protocol first needs a `send`.

The table ends at the first blank line. Every line before that must be a row; a row-like line
right after the blank line is refused, so a stray blank line cannot drop rows. Tables inside
fenced code blocks (like the one above) are ignored.

**Shape** (WIRE.md, Layout in a message). A message is **inline** if its request's fields
and its reply's fields each have a fixed size (no `string` or `bytes`) and fit in 12 bytes (words 1-3 at 32 bits, the same on
both widths); the fields are packed into words 1-3 and there is no buffer. Otherwise it is a
**buffer** message: the request's fields go in the buffer (a lend when sent with `call`, a
transfer with `send`) with their length in word 1, and the reply's fields are written back
into the caller's lend with their length in word 1. A small request whose reply carries data
(a block read) is therefore a buffer message.

**The error table.** Every protocol has one, marked `<!-- wire-errors: NAME -->` (same NAME)
with the header `| Code | Error |`: one row per error, the code decimal, unique, the name
snake_case in backticks and unique. Code 0 is success, and **code 1 is `malformed` in every
protocol** (WIRE.md, Errors): a request that does not decode (unknown opcode, wrong shape, bad
lengths, a missing handle, or one found to be of the wrong kind). The generator adds it to
every table, so a protocol's own codes start at 2, and a table that lists code 1 or the name
`malformed` itself is refused. A protocol with no errors of its own writes the header and
separator alone. An error reply carries the code in word 0, zeros in words 1-3 and no
handles; the caller ignores the buffer.

**In a 9P file.** A message written into a file (e.g. an `ipd` `ctl` file) is its opcode as a
`u32` followed by the buffer-shape encoding of its fields, one per `Twrite`. Messages with
handles cannot be written into a file.

**Generating.** `cargo run -p redoubt-wire-gen` writes `libs/wire/src/proto/NAME.rs` and
`libs/wire/elixir/proto/NAME.ex`. The generated files are checked in, and
`cargo test -p redoubt-wire-gen` fails if they differ from the tables
(`cargo run -p redoubt-wire-gen -- --check` says which), so a table and its code cannot drift.

## The tables

<!-- wire: example -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `ping` | - | - |
| 2 | `pong` | `seq: u64`, `flags: u32` | - |
| 3 | `small` | `a: u8`, `b: u16` | `c: u32` |
| 4 | `wide` | `a: u64`, `b: u32`, `c: u8` | - |
| 5 | `named` | `id: u32`, `name: string` | `id: u32` |
| 6 | `blob` | `offset: u64`, `data: bytes`, `label: string` | - |
| 7 | `grant` | `range: handle[0] endpoint`, `reply: handle[1] endpoint`, `pages: u32` | `key: handle[0] budget` |
| 8 | `read` | `offset: u64`, `count: u32` | `data: bytes` |
| 4294967295 | `last` | `note: string`, `key: handle[0] process` | `n: u64`, `m: u32` |

<!-- wire-errors: example -->
| Code | Error |
| --- | --- |
| 2 | `not_found` |
| 3 | `denied` |
| 4294967295 | `last_error` |
