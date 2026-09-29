# sshd's vendored crates under Miri

## What

`sshd`'s SSH library, `sunset`, and the crates it brings are vendored and pinned
([vendored dependencies](../testbench.md#vendored-dependencies)), but their `unsafe` has only been
counted, not read or run under Miri as `ipd`'s crates were ([ipd under Miri](../servers/ipd.md#under-miri)).
`sunset` itself has none. What the box compiles of the rest is the software backends of `aes`,
`chacha20`, `poly1305` and `sha2`, and the helpers under them: `inout`, `block-buffer`,
`hybrid-array`, `cmov`'s portable path, `zeroize`, `subtle`, `ascii` and `getrandom`'s custom
backend (vendor/README.md, "sshd's SSH library").

## Why it matters

`sshd` parses bytes from the whole network before any login, and these crates run inside it.
Vendored code sits outside the unsafe ratchet on the condition that its `unsafe` is read and its
tests are run under Miri on the paths the box runs.

## Where

- `vendor/` (the crates), and each crate's own tests in an unedited copy outside the tree, with
  its software backend forced: `aes_backend="soft"`, `chacha20_backend="soft"`,
  `poly1305_backend="soft"`, `sha2_backend="soft"`.
- A record like `ipd`'s, in `sshd`'s page.

## Done when

The `unsafe` the box compiles in each crate is read and recorded in vendor/README.md, as
`heapless`'s is, and each crate's tests have a recorded Miri run, with anything that cannot run
under Miri named.
