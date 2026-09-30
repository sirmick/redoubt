# Invisible format characters pass the terminal guard

## What

[The terminal guard](../userland/shell.md#hostile-text-never-drives-the-terminal) draws every
control character as text, but its list stops at the ASCII and 8-bit controls, DEL and the
bidirectional embedding, override and isolate controls. These pass it and are drawn as they are:

- the invisible format characters: U+00AD (soft hyphen), U+200B to U+200D (zero-width space,
  non-joiner and joiner), U+2060 to U+2064 (word joiner and the invisible operators), U+FEFF (the
  byte order mark), and the tag characters, the block of 128 at the start of plane 14;
- the line and paragraph separators, U+2028 and U+2029.

## Why it matters

None of them drives the terminal, and none reaches the approval channel, which only the steward
draws. But a file name, a program's output or a model's reply that holds them can look like
another: `rm report.txt` and `rm re\u{200b}port.txt` are drawn the same, and a tag sequence can
carry text the person never sees. The guard's purpose is that what the person sees is what is
there.

## Where

- `userland/native/cells/src/lib.rs`: `forbidden()`, the characters no symbol may hold, and
  `no_control_character_is_ever_a_symbol` in `src/tests.rs`, which today counts U+2028 as a good
  symbol.
- `userland/shell/lib/redoubt/term/text.ex`: `control?/1`, the characters the printer draws as
  text, with its tests.
- `userland/native/cells/vectors.json`: a vector for each, so the two decoders agree.

## Done when

The characters above are drawn as `<U+XXXX>`, as U+009B is, by the printer and never held by a
cell's symbol. A vector and a test for each hold both decoders to it, and the residual in the
shell's page and in [the security summary](../SECURITY.md) goes.

Joiners inside an emoji sequence (U+200D in `👩‍💻`) are one grapheme and must still draw as one:
the rule for U+200D needs deciding first, either as "inside a grapheme only" or as "never".
