# A shared image cache

## Idea

Program images kept read-only in one cache and mapped into every process that runs them, instead
of each child paying for its own copy.

## Why it is not a goal

It is ruled out as a channel. A cache shared between principals tells one principal, by how fast a
launch goes or what it costs, which programs another has run. Today every child pays for a copy of
its image: the launcher copies the ELF into pages charged to the child, and the loader stub copies
each segment again ([init](../servers/init.md#residual-risks)). That costs memory and launch time,
and leaks nothing.

## What it would need

- The owner's decision that the saving outweighs the channel.
- A cache per (account, label set), so it is shared only inside one trust domain, where nothing is
  separated anyway; that keeps the saving for a principal's own repeated launches only.
- Charges that do not depend on whether the image was already cached.

**Attack cases:** a launch's time and cost are the same whether or not another principal ran the
same program; an image page can never be written or executed through a second mapping
([R11 (memory)](../kernel/memory.md#r11-memory)).
