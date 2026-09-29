# Other runtimes

## Idea

Python and Java, ported to Rust, so that code written for them runs on Redoubt. Slow is
accepted.

## Why it is not a goal

The shell and the session are Elixir, on beamlet, and a second and third language runtime are each
a large body of code to audit. The owner's rule stands for them as for everything else: no C, so
the reference interpreters and virtual machines cannot be used as they are, and a port is a
project of its own.

A small embedded script language was weighed too, and not taken: Elixir on beamlet is the box's
scripting language, and the one small candidate that builds `no_std`, Rhai, carries `unsafe` of
its own and in dependencies it cannot drop. The client library binds any language, so one can
be added later without changing it ([the client library](../userland/native.md#the-client-library)).

## What it would need

- **A runtime in Rust only**, with the same audit rules as beamlet: no C, no foreign-function
  interface, no native extension modules; foreign code runs as a separate program
  ([beamlet](../userland/beamlet.md)).
- **A platform boundary** like beamlet's `Platform`, over the client library: files through the
  namespace over 9P, the network only through capabilities, time and randomness from the kernel,
  nothing ambient.
- **Each program in a budget of its own**, launched like any native program, with only the
  capabilities its launcher binds ([native programs](../userland/native.md)).
- **Hostile code contained inside the runtime** only as far as the runtime is memory-safe; the
  budget is the real wall.

**Attack cases:** a script cannot reach a path or a network its namespace lacks; a runaway script
ends at its budget's limits; a hostile module cannot load native code.
