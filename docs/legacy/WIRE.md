# Wire formats

Designed; the codecs are built (`libs/wire/`, WP-W1). Owns: how bytes are laid out in messages
and in human-written files. One convention for every message, one for every file a person writes.

## Messages: 9P's convention
- **9P** is plain 9P2000 (no `.u` or `.L` extensions) with a fixed `msize` of 64 KiB, which is
  `MAX_LEND_PAGES` (KERNEL-SPEC.md). A 9P message travels in a lent buffer, and the call's four
  words are all zero, in the request and in a successful reply. A request with word 0 = 0 and any
  other non-zero word, or with no lend, is refused with reply status 1 (`Malformed`, below). A
  request whose word 0 is not 0 is a typed operation on the same endpoint: every 9P server serves
  `ninep_common` (`new_connection`, `disconnect`; NAMESPACES.md).
- **Opcodes on a 9P endpoint.** `ninep_common` reserves **opcodes 1-15** there, so it can grow
  without colliding, and a server's own protocol on the same endpoint starts at **16**. A table
  marked as a 9P server's protocol (`<!-- wire: NAME ninep -->`) that uses an opcode below 16 is
  refused by the generator (question 113). A protocol on an endpoint of its own is unaffected and
  starts at 1.
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
Each protocol's table lives in the note of the server that serves it (README.md, topic map), written
by that server's work package with an approval record in ANSWERS.md (BUILD-PLAN.md). A line holding only
`<!-- wire: NAME -->` names the protocol; the next table is its layout. A protocol served on a 9P
endpoint is marked `<!-- wire: NAME ninep -->`, and its opcodes start at 16, since `ninep_common`
reserves 1-15 there (above):

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
- **Typed messages are `call`s unless a table's `Kind` column says `send`.** A protocol that
  needs a `send` (a transfer) heads its table `| Opcode | Kind | Message | Fields | Reply |` and
  gives each row `call` or `send`. A `send` has no reply (`-`): it is never an open call, so
  nothing can answer it. A table without the column is all calls. `ipd`'s ingress `frame` is the
  first `send` (IO-ARCHITECTURE.md, Networking; answer 174). The generator keeps its tables
  uniform, so a `send` row still gets an empty reply type and a `REPLIES` layout; no receiver
  encodes one, because there is no open call to answer (`is_send()` says which rows these are).
- **Errors.** Each protocol has an error table, marked by a line `<!-- wire-errors: NAME -->` and
  headed `| Code | Error |`: codes unique, each with a name. **Code 1 is `Malformed` in every
  protocol**, and in a 9P call's reply status: a request that does not decode (unknown opcode, wrong
  shape, bad lengths, a missing handle, or one found to be of the wrong kind). The generator
  reserves it and adds it to every table; a protocol's own codes start at 2, and a protocol with no errors of its own has an error table with no rows.

### Granting and releasing
A typed protocol that mints a narrower capability **names its grant and release operations**, the
typed counterpart of 9P's `new_connection` and `disconnect` (NAMESPACES.md, `ninep_common`). Stated
once here, so no server invents its own shape:
- **`grant`** mints a capability **no wider than the caller's own** — never a right the caller does
  not hold, never a wider one — **stamped like the handle the request came in on** (CAPABILITIES.md,
  stamps), and returns a **random id**: unpredictable, 64-bit, never a counter.
- **`release(id)`** frees that capability **and everything granted under it**, and only for the
  holder of the id: an id the caller never received is refused exactly as one that does not exist,
  so nothing is revealed (as `disconnect`'s `not_yours`, NAMESPACES.md).

Each protocol writes the two rows into its own table, with its own fields (`keyd`'s `grant` names a
key and a purpose); only the shape and the rules are common. A launcher releases a child's grants
when it receives the child's exit notice, as it disconnects its connections (CAPABILITIES.md;
INIT.md, launching).

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
- **The startup block** (INIT.md) is one typed message laid out the same way in its page, behind a
  `u32` byte length: the length, the opcode as a `u32`, then the buffer-shape encoding of its
  fields. The length is what lets it be read out of a page at all, since a typed message has no
  overall length and the decoder refuses trailing bytes (question 112). It is decoded by
  `redoubt-wire` like any other message.

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
