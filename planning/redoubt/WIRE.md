# Wire formats

Designed; the codecs are built (`redoubt/wire/`, WP-W1). Owns: how bytes are laid out in messages
and in human-written files. One convention for every message, one for every file a person writes.

## Messages: 9P's convention
- **9P** is plain 9P2000 (no `.u` or `.L` extensions) with a fixed `msize` of 64 KiB, which is
  `MAX_LEND_PAGES` (KERNEL-SPEC.md). A 9P message travels in a lent buffer, and the call's four
  words are all zero, in the request and in a successful reply. A request with word 0 = 0 and any
  other non-zero word, or with no lend, is refused with reply status 1 (`Malformed`, below). A
  request whose word 0 is not 0 is a typed operation on the same endpoint: every 9P server serves
  `ninep-common` (`new_connection`, `disconnect`; NAMESPACES.md).
- **Typed messages** (everything that is not 9P: `blkd` <-> `fsd`, the steward, `keyd`, `sshd`
  <-> steward, `ipd`'s connect and listen operations) use **9P's own encoding**: little-endian
  fixed-size integers (`u8`, `u16`, `u32`, `u64`), strings as `u16` length + UTF-8, byte arrays as
  `u32` length + bytes.
- **One layout per message type**, defined by a table in the owning server's note (below). The Rust
  and Elixir codecs are generated from those tables (`redoubt-wire-gen`), and the generated code
  fails its test when it drifts from the notes, so sender and receiver cannot disagree. There is no
  self-describing format and no text command parser.
- **One codec**, shared by 9P and the typed messages, and fuzzed.

### Tables
Each protocol's table lives in the note of the server that serves it (README.md, Servers), written
by that server's work package with a HISTORY.md line (BUILD-PLAN.md). A line holding only
`<!-- wire: NAME -->` names the protocol; the next table is its layout:

```
<!-- wire: example -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `read` | `block: u64`, `count: u32` | `data: bytes` |
| 2 | `grant` | `range: handle[0] endpoint`, `pages: u32` | - |
```

- One row per message type: the opcode (decimal, unique; **0 is reserved**, since word 0 of a reply
  is a status and 0 means ok), the name, the request's fields in order, and the reply's fields in
  order. Each field is `` `name: type` ``; `-` means none (a reply of `-` is its status alone).
- **Types:** `u8`, `u16`, `u32`, `u64`, `string` (`u16` length + UTF-8), `bytes` (`u32` length +
  bytes), and `handle[N] KIND` (the handle in slot N; slots are numbered from 0 in order, carry no
  bytes, and a message has at most `MAX_MSG_HANDLES`). `KIND` is the object the handle must name:
  `endpoint`, `budget`, `process`, `mmio`, `irq` or `reset` (KERNEL-SPEC.md, Objects). The kind is
  documentation, put in the codec's docs by the generator: the kernel does not report a received
  handle's kind, so a handle of the wrong kind is found by use (`WrongObject` on first use).
- **Compound values** (a label set, an IP prefix) are a `bytes` field whose inner layout is stated
  under the table, in the same encoding. Milestone 1 adds no other types.
- **Every milestone 1 typed message is a `call`.** A table has no column saying so; a `kind` column
  is added when a protocol first needs a `send` (a transfer).
- **Errors.** Each protocol has an error table, marked by a line `<!-- wire-errors: NAME -->` and
  headed `| Code | Error |`: codes unique, each with a name. **Code 1 is `Malformed` in every
  protocol**, and in a 9P call's reply status: a request that does not decode (unknown opcode, wrong
  shape, bad lengths, a missing handle, or one found to be of the wrong kind). The generator
  reserves it and adds it to every table; a protocol's own codes start at 2.

### Layout in a message
- **Word 0** of a request is its opcode. **Word 0 of a reply is its status**: 0 = ok, otherwise a
  code from the protocol's error table; a reply with a non-zero status carries no fields.
- **Two shapes, fixed per message type by its table.** A message is **buffer-shaped** if its request
  or its reply has a `string` or `bytes` field or does not fit 12 bytes; otherwise it is
  **inline**.
  - **Inline:** the fields pack 4 bytes per word into words 1-3, little-endian, zero-padded (12
    bytes). Every word fits 32 bits, so the layout is the same on both widths. A reply's fields go
    in its words 1-3 the same way.
  - **Buffer:** the request's fields (only the fields: the opcode stays in word 0) are encoded in
    the buffer (a `call`'s lend or a `send`'s transfer), and word 1 holds their length; words 2 and
    3 are 0. The reply's fields are written into the caller's lend, with their length in the
    reply's word 1. The kernel's `reply` carries only words and handles, so reply data can travel
    only there.
- **Typed operations written into a 9P file** (`ipd`'s `/net/tcp/N/ctl`, NAMESPACES.md): a file's
  contents have no words, so each operation is one `Twrite` whose data is the opcode as a `u32`
  followed by the buffer-shape encoding of its fields.
- **The startup block** (INIT.md) is one typed message laid out the same way in its page: the
  opcode as a `u32`, then the buffer-shape encoding of its fields. It is decoded by `redoubt-wire`
  like any other message.

## Files people write: strict JSON
The boot manifest (INIT.md), package manifests (PACKAGES.md) and configuration are JSON under the
I-JSON profile (RFC 7493), enforced by one shared parser:
- UTF-8 only; no duplicate member names; member names are compared byte for byte, with no Unicode
  normalisation;
- **each field's JSON type is fixed by its file's schema**: 64-bit quantities (ids, accounts,
  labels, addresses, byte and page sizes, deadlines) are always decimal strings, and small counts
  (weights, depths, restart limits) are always numbers; a value of the wrong JSON type is an error;
- nesting at most 32 deep; a file at most 64 KiB;
- unknown members are errors, not ignored.
