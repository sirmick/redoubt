# Development on Redoubt

Redoubt is developed on Redoubt. A developer, or a developer's agent, works in a session: the source
lives in their home volume, `git` reaches its remotes through a gateway, and the Elixir and Erlang
compilers run on the box in beamlet. Rust, including the kernel and the servers, is built off the
box, because the Rust compiler is not ported: system code ships in the signed boot bundle, and a
developer's own program arrives by SFTP. The server APIs and client crates let programs written for
the box use the system; whether Rust's `std` gets a Redoubt target is open
([native programs](native.md#client-crates-and-the-rust-std-target)).

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
iex(2)> git pull                                 # through a gatewayd git capability
iex(3)> ed "userland/lib/redoubt/shell.ex"
iex(4)> mix test                                 # the Elixir compiler and ExUnit, in the session
iex(5)> git commit -am "shell: complete labels" ; git push
```

Rust is built on the developer's own machine. A program for one's own use arrives by SFTP and runs
unsigned, then and later. A program the steward is to launch with new grants, for another
principal, arrives from M5 (persist, install, share) as a signed package:

```text
laptop$ cargo build --release --target riscv64gc-unknown-redoubt-elf
laptop$ xpkg build && xpkg sign --key alice        # -> logscan-1.3.xpkg
laptop$ scp logscan-1.3.xpkg alice@box:/home/alice/in/
iex(6)> pkg add "/home/alice/in/logscan-1.3.xpkg"
```

## What it can and cannot do

### The compilers on beamlet

Status: built · partly tested: runs on the host only; the compiler cases (`tests/elixir/compiler_test.ex`, `tests/erlang/selfcompile.erl`) are differential suites that need OTP 28 and Elixir installed and are not run by the bench, and they only compile, load and call: nothing checks the chunk equality or the error handler below

Elixir's compiler (`Code.compile_string`, `Code.eval_string`) and OTP's Erlang compiler
(`compile:forms`) run on beamlet, and the modules they produce load and run in the same VM
([`userland/otp/tests/elixir/compiler_test.ex`](../../userland/otp/tests/elixir/compiler_test.ex),
[`userland/otp/tests/erlang/selfcompile.erl`](../../userland/otp/tests/erlang/selfcompile.erl)).
The code, atom, export and literal chunks they write are identical to BEAM's; compressed chunks
(debug information, documentation) differ in bytes, because the deflate implementation differs,
and decode to the same terms. A call to a function not yet loaded goes to the process's error
handler, which Elixir's parallel compiler uses to wait for modules
([`userland/otp/DESIGN.md`](../../userland/otp/DESIGN.md)).

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

`git` reaches its remotes through a gateway: a `gatewayd` capability for `git`, the documented path
for people and agents alike ([gatewayd](../servers/gatewayd.md)).
- **Scoped by name and by operation.** The capability names its remotes; fetch and push are granted
  separately; push is limited to named ref patterns; force-push is refused unless granted.
- **No credentials in the session.** `gatewayd` holds the remote's credentials (a token or a deploy
  key), terminates TLS, checks each request and logs it. The client never sees a credential.
- **The client is a Rust `git`.** Redoubt has no C, so the client is a pure-Rust implementation,
  built off the box and shipped in the system bundle like other system Rust. It runs as a native
  program in a budget carved from its launcher's (a session or an agent), holding only the working
  tree it was given and one remote capability; the gateway speaks git's smart HTTP to it. Ctrl+C or
  the end of a lease destroys it like any native stage ([native programs](native.md)).
- **Agents get only this path**, since they never get sockets. A person may instead hand the client
  their own name-scoped TCP capability to an allowlisted remote, with credentials from their own
  session: that is their general network right, not the development path.

**Open:** the gateway's finer checks on `git` requests beyond remote, operation, refs and force
(size limits, path rules), which belong to [gatewayd](../servers/gatewayd.md).

### Rust built off the box

Status: planned · M4 (self-hosted development)

The Rust compiler is not ported, so native programs, servers and the kernel are built off the box
with the `riscv64gc-unknown-redoubt-elf` target, against the client crates ([native
programs](native.md#client-crates-and-the-rust-std-target)). "Shipped signed" means signed where a
signature gates something:
- **System Rust** (servers, drivers, beamlet, the kernel) reaches the box in the signed boot
  bundle, checked by verified boot ([boot](../kernel/boot.md)).
- **A developer's own program** arrives by SFTP ([file transfer](transfer.md)) and runs with the
  developer's own authority, unsigned. Code never runs with more authority than its author holds,
  and any process can create a child and map pages into it, so a signature on code one runs
  oneself buys nothing enforceable ([native programs](native.md#launching-from-a-session)).
- **A program the steward launches with new grants**, for another principal or as a package,
  needs a signature the principal trusts. That, with trust lists and packages, is
  M5 (persist, install, share) ([packages](packages.md)).

**Open:** none.

### Signing keys

Status: planned · M5 (persist, install, share)

The box only checks signatures against trust lists; it does not care where a private key lives.
- **A person may sign off the box** with their own key, and that is always valid.
- **`keyd` may also hold a person's package-signing key**, as its own key with the purpose `pkg`
  and its own domain-separated preimage ([keyd](../servers/keyd.md)). The rule that keys which
  authenticate a person never live in `keyd` covers login and approval keys, not this one, and
  `keyd` still refuses a key enrolled both as a login or approval key and as one of its own.
- **Every signature with a person's key in `keyd` needs an approval** at `approve@`, showing the
  package's digest, name and manifest requests. That signature vouches to everyone who trusts the
  key, so a hijacked session must not become a signing oracle for its owner.
- **An agent signs with its own key in `keyd`**, without an approval: its code runs only within its
  lease until a sponsor trusts its key, and that trust is itself a high-stakes approval
  ([agents](agents.md)).

**Open:** none.

## Why

**Develop where the walls are.** A capability system is easy to make safe and hard to make
usable; the only test of usability is use. Developing Redoubt on Redoubt makes its developers
meet every wall daily, and makes their agents the first real tenants.

**Elixir on the box, Rust off it.** The Elixir and Erlang compilers are Erlang and Elixir code,
and beamlet already runs them. The Rust compiler is millions of lines with an LLVM back end in
C++, which Redoubt will not carry: no C, and no C++, anywhere in the system. Building Rust
elsewhere keeps the box's trusted code small while still running native code written for it.

**Signatures for grants, not for running.** Requiring a signature to run any code would stop
nothing, because a session can already run any Elixir it writes, and would make development
slower. Signatures matter where the steward adds authority: that is where "who wrote this" has to
be answered.
