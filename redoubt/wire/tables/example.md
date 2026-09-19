# Example protocol (codec conformance fixture)

Not a real protocol: no server speaks it. It exists so the generated codecs, the test vectors
(`redoubt/wire/vectors/example.txt`) and the fuzz target exercise every field type and both
message shapes before the servers' own tables exist. Real tables live in the owning server's
note under `planning/redoubt/` (WIRE.md) and use exactly this format; `redoubt-wire-gen` reads
both places.

The format: an HTML comment `<!-- wire: NAME -->` on its own line names the protocol, and the
next table is its layout. One row per message type: the opcode (decimal, unique), the message
name, and the fields in order, each `` `name: type` ``, or `-` for none. Types: `u8`, `u16`,
`u32`, `u64`, `string` (`u16` length + UTF-8), `bytes` (`u32` length + bytes), and
`handle[N]` (the handle in slot N; slots are numbered from 0 in order and carry no bytes).
Whether a message is inline or goes in a buffer follows from its fields (redoubt-wire's
`typed` module).

<!-- wire: example -->
| Opcode | Message | Fields |
| --- | --- | --- |
| 1 | `ping` | - |
| 2 | `pong` | `seq: u64`, `flags: u32` |
| 3 | `small` | `a: u8`, `b: u16` |
| 4 | `wide` | `a: u64`, `b: u32`, `c: u8` |
| 5 | `named` | `id: u32`, `name: string` |
| 6 | `blob` | `offset: u64`, `data: bytes`, `label: string` |
| 7 | `grant` | `range: handle[0]`, `reply: handle[1]`, `pages: u32` |
| 4294967295 | `last` | `note: string`, `key: handle[0]` |
