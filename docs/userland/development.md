# Development on Redoubt

Redoubt is developed on Redoubt. A developer, or a developer's agent, works in a session: the
source lives in their home volume, `git` reaches its remotes through a gateway, and the Elixir and
Erlang compilers run on the box in beamlet. Rust, including the kernel and the servers, is built
off the box, because the Rust compiler is not ported, and shipped signed. The server APIs, the
client crates and a Rust `std` target exist so that programs written on the box can use the
system.

## Purpose

A system that is only ever built elsewhere has never been used for real work by the people who
know it best. Developing Redoubt on Redoubt puts its own developers and their agents inside the
walls: every missing tool, awkward API and wall that is too tight or too loose shows up in daily
use. It is also the first serious workload for agents on the box, which is what the system is for.

## How to use it

A working day, sketched:

```text
$ ssh alice@box
iex(1)> cd "/home/alice/redoubt"
iex(2)> git pull                                 # through the gateway, to an allowlisted remote
iex(3)> ed "userland/lib/redoubt/shell.ex"
iex(4)> mix test                                 # the Elixir compiler and ExUnit, in the session
iex(5)> git commit -am "shell: complete labels" ; git push
```

Rust is built on the developer's own machine. With packages, from M5 (persist, install, share),
it arrives as a signed package:

```text
laptop$ cargo build --release --target riscv64gc-unknown-redoubt-elf
laptop$ xpkg build && xpkg sign --key alice        # -> logscan-1.3.xpkg
laptop$ scp logscan-1.3.xpkg alice@box:/home/alice/in/
iex(6)> pkg add "/home/alice/in/logscan-1.3.xpkg"
```

## What it can and cannot do

### The compilers on beamlet

Status: built · partly tested: runs on the host only; the compiler cases (`tests/elixir/compiler_test.ex`, `tests/erlang/selfcompile.erl`) are differential suites that need OTP 28 and Elixir installed and are not run by the bench

Elixir's compiler (`Code.compile_string`, `Code.eval_string`) and OTP's Erlang compiler
(`compile:forms`) run on beamlet, and the modules they produce load and run in the same VM
([`userland/otp/tests/elixir/compiler_test.ex`](../../userland/otp/tests/elixir/compiler_test.ex),
[`userland/otp/tests/erlang/selfcompile.erl`](../../userland/otp/tests/erlang/selfcompile.erl)).
The code, atom, export and literal chunks they write are identical to BEAM's; compressed chunks
(debug information, documentation) differ in bytes, because the deflate implementation differs,
and decode to the same terms. A call to a function not yet loaded goes to the process's error
handler, which Elixir's parallel compiler uses to wait for modules
([beamlet](beamlet.md#what-runs-on-it)).

### Compiling on the box

Status: planned · M4 (self-hosted development)

`mix compile` and `mix test` run in the developer's session, in its VM, with its authority. Source
and build output are files in the developer's volumes; the `.beam` files a build writes load into
the session through the VM's code path, which grants nothing, since a session can already load
any bytes it holds ([beamlet](beamlet.md#the-platform-boundary)). A build that should not run with
the whole session's authority (an untrusted dependency's code, a test suite from someone else)
can run in a child VM with its own budget and a narrower namespace, as an agent does
([agents](agents.md)).

Code a developer compiles runs within their own authority and needs no signature. Code that is to
run with authority the steward grants (for another principal, or as a package) is signed
([packages](packages.md)).

**Open:** none.

### `git` through a gateway

Status: planned · M4 (self-hosted development)

`git` reaches its remotes through a gateway, never through an open socket, so a session or agent
that holds the capability for one remote can push to that remote and nowhere else. The network's
rules apply as everywhere: default deny, allowlists written as names, the name resolved by the
mediated resolver and the connection pinned to it ([the resolver](../servers/resolver.md)). The
box's own addresses and the host's are never reachable.

**Open:** which path `git` takes and which client runs it. The recommendation: people get `git`
over name-scoped TCP to allowlisted remotes, and agents through a `gatewayd` capability for `git`
scoped to named remotes ([gatewayd](../servers/gatewayd.md)); and the client is a native program
in Rust, since Redoubt has no C.

### Rust built off the box

Status: planned · M4 (self-hosted development)

The Rust compiler is not ported, so native programs, servers and the kernel are built off the box
with the `riscv64gc-unknown-redoubt-elf` target and the Rust `std` target, against the client
crates ([native programs](native.md#client-crates-and-the-rust-std-target)). The binary arrives
signed: the signature says who vouches for the code, and the steward launches it with new grants
only for a principal who trusts the signer. A binary launched by its own developer, with a subset
of the developer's own handles, needs no signature: it gains nothing the session did not have
([native programs](native.md#launching-a-program)).

**Open:** how a Rust program built off the box reaches the box and runs before packages exist
(packages are M5 (persist, install, share)), and where a developer's signing key lives. The
recommendation: in M4 (self-hosted development) a binary arrives by SFTP and runs with the
developer's own authority, needing no signature, and signed packages follow in
M5 (persist, install, share); a person signs off the box with their own key, which never lives in
`keyd`, and an agent signs with a key `keyd` holds for it.

## Why

**Develop where the walls are.** A capability system is easy to make safe and hard to make
usable; the only test of usability is use. Developing Redoubt on Redoubt makes its developers
meet every wall daily, and makes their agents the first real tenants.

**Elixir on the box, Rust off it.** The Elixir and Erlang compilers are Erlang and Elixir code,
and beamlet already runs them. The Rust compiler is millions of lines with an LLVM back end in
C++, which Redoubt will not carry: no C, and no C++, anywhere in the system. Building Rust
elsewhere and shipping it signed keeps the box's trusted code small while still running native
code written for it.

**Signatures for grants, not for running.** Requiring a signature to run any code would stop
nothing, because a session can already run any Elixir it writes, and would make development
slower. Signatures matter where the steward adds authority: that is where "who wrote this" has to
be answered.
