# Reading this book

Redoubt is a headless, multi-user operating system for RISC-V, built so that hostile agents can
run on it, do real work, and not get out. This book describes the whole system from its code:
what each part does, which rule it keeps, which test attacks that rule, and what is still
planned. The [tenets](TENETS.md) outrank every other page.

## Reading order

1. [The tenets](TENETS.md): what Redoubt is, the threat model, the guarantees and the walls that
   hold them, the non-goals.
2. [The glossary](GLOSSARY.md): the one vocabulary every page uses. Skim it once; come back to it.
3. [The kernel](kernel/README.md): handles, IPC, memory, budgets, scheduling, the timer,
   processes, devices, boot, and the model that checks them.
4. [The servers](servers/README.md): trust tiers and labels, then one page per server, from
   `init` and the steward to the network path.
5. [Userland](userland/README.md): what a person, an agent and a developer see.
6. [The security register](SECURITY.md): every security property on one table, with the code
   that enforces it, the test that attacks it and its residual risks. An auditor starts here.
7. [The plan](plan/m1-separation.md): one page per milestone, then the follow-ups in `todo/` and
   the ideas past the plan in `beyond/`.
8. [The test bench](testbench.md): how every claim in this book is tested.

To build and run Redoubt, see
[GETTING-STARTED.md](../GETTING-STARTED.md). How the work itself is organised is in
[SWARM.md](SWARM.md) and [PROJECT.md](PROJECT.md).

## Written as if complete

Every page describes the system as it will be at the end of the plan. Whether a part exists
today is said by the **status line** at the top of its section, never by the prose. A section
that is part built and part planned is split in two.

| Status line | Meaning |
| --- | --- |
| `Status: built · tested: <tests>` | The code exists, and each named test attacks the section's claim. |
| `Status: built · partly tested: <gap> · tested: <tests>` | The code exists; the gap says what no test attacks yet, and the tests (when named) cover the rest. |
| `Status: planned · M2 (usable shell)` | Nothing of it is built; it arrives in the named milestone. The section ends with an **Open:** list of what is still undecided. |

"Built" means the code exists and its named tests pass. A host test or a build case is not a
boot: code built and host-tested is not thereby running in the integrated system, and running
there is its own planned section.

A section without its own status line inherits its nearest parent's. Purpose, Residual risks and
Why sections carry none: they say why and what is left, not what is claimed. Status lines appear
on the kernel, server and userland pages and on the test bench page; plan, follow-up and idea
pages do not use them, and the security register has a status column instead.

A test is named by its kind:

| Form | What it is |
| --- | --- |
| `bench:<case>` | a bench case, `tests/<case>.toml`: a real boot under QEMU, a host-test run or a source check ([the test bench](testbench.md)) |
| `host:<crate>::<test>` | a Rust `#[test]` in that crate, run on the build machine; the model's properties are host tests of `redoubt-model` |
| `mutation:<name>` | a rule deliberately broken in the model, which its checks must catch ([the model](kernel/model.md)) |
| `fuzz:<crate>/<target>` | a fuzz target in that crate's `fuzz/` directory |

## The milestones

A milestone is always written with its name.

| Milestone | Goal |
| --- | --- |
| [M1 (separation and containment)](plan/m1-separation.md) | Alice and Bob log in over SSH into Elixir sessions and are kept apart; Alice's agent runs contained under a lease; the attack suite passes. |
| [M2 (usable shell)](plan/m2-usable-shell.md) | The Elixir shell is a working environment: command mode, file operations, native programs and pipes, jobs, line editing, the editor. |
| [M3 (files in and out)](plan/m3-files.md) | SFTP and SCP inside SSH, confined to the session's capabilities and audited. |
| [M4 (self-hosted development)](plan/m4-self-hosted.md) | Redoubt is developed on Redoubt: compilers, `git` and a model provider through gateways, the agent harness, the audit log and the escape room. |
| [M5 (persist, install, share)](plan/m5-persist.md) | The steward's state survives reboots; signed packages, trust lists and shared projects; A/B updates; the supervisor; wall-clock time; log retention. |

Anything past M5 (persist, install, share) is **beyond M5**: an idea, not a goal, one page each
under [beyond/](beyond/README.md).

## Rule IDs

Every security property has an ID, defined once by a heading on the page that owns it and cited
everywhere else with its short name the first time on a page:

- **Rules** are one series, `R` and a number. The kernel's come first, on the kernel pages, such
  as [R3 (lends and abandoned calls)](kernel/ipc.md#r3-lends-and-abandoned-calls); the server,
  network and policy rules take the numbers after them, on the server pages, such as
  [R25 (the label check)](servers/serving.md#r25-the-label-check). A letter after the number
  refines a rule: [R4a (open calls)](kernel/ipc.md#r4a-open-calls).
- **Invariants** are `I` and a number, all on [invariants](kernel/invariants.md), such as
  [I9 (pages W^X, zeroed, lends unmapped)](kernel/invariants.md#i9-pages-wx-zeroed-lends-unmapped).
  The model checks every one after every step.
- **Rule F** (trusted verdicts) is the bench's own: a verdict comes only from a party the attacker
  cannot impersonate ([the test bench](testbench.md#rule-f-trusted-verdicts)).

IDs are cited one at a time, never as ranges, and never reused: a withdrawn rule keeps its
heading, marked withdrawn. The [security register](SECURITY.md) lists every ID.

## Rendering

The pages are plain Markdown and read on GitHub as they are. [mdBook](https://rust-lang.github.io/mdBook/)
renders them as a book with navigation and search:

```sh
mdbook build docs      # output in target/book
mdbook serve docs      # the same, served on localhost with live reload
```

Diagrams are text inside the pages: Mermaid for flows, sequences and state machines, svgbob for
memory maps and box layouts. Solid lines are built; dashed lines are planned. The book needs the
`mdbook-mermaid` and `mdbook-svgbob` preprocessors. The docs checker, `redoubt-doccheck`, holds
every page to these rules ([the test bench](testbench.md#the-docs-checker)).
