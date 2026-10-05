# BEAM7 launch brief and BEAM3 prerequisites

BEAM7's bounded implementation scope is ready under the settled R75 rule. The Architect
has made no code, test or book edits. SCHED1's fixture review takes priority when it arrives.
This brief does not authorize an implementation of the whole BEAM3 plan node.

## Recommendation

First deliver **terminal refusal at the Platform lookup boundary**, as a separate Tier A
package BEAM7. A verified module or application lookup has three
outcomes: found, absent, refused. Only absent permits module code-path fallback. This is
already required by R75 and `docs/todo/beamlet-refused-module-falls-through.md`; no new
owner choice or authority is needed. Do not enable Redoubt `Platform::files` in this package.

Start from current main in its own worktree. It needs BEAM2, which is done; neither SCHED1
nor MEM1 is a semantic dependency. Reserve the testbench host-case support paths with the
orchestrator before assigning concurrent edits. Do not mark BEAM3 done when this lands:
its existing scope is asynchronous console/native work and files on /boot and fsd.

Plan coordination: the orchestrator has already created and started BEAM7 under
beamlet-redoubt with needs `[BEAM2]`; do not create a duplicate or edit that started node.
For the orchestrator, BEAM3's immediate dependency update is to preserve BEAM2 and AIO1
and add BEAM7. This brief does not edit the started BEAM3 node. Add the separately named
client prerequisites below to its dependencies when their nodes are created.

## What is built, and what blocks broader work

`plan_get BEAM3` names BEAM2 and AIO1 as prerequisites, both done, and explicitly requires
the refused-module fix before files. The following gaps are evidenced in current source:

| Gap | Source and required action |
| --- | --- |
| Refusal becomes absence | `Platform::load_module/load_app` return `Option`; Redoubt's `load` maps both `Unloaded::Absent` and `Unloaded::Refused` to `None`; `System::locate_module` then searches files. Fix first. |
| Dropped fids never return | `libs/client/src/file.rs` documents and implements retention until connection end. Implement native.md's last-reference/in-flight-aware deferred clunk before Erlang process death can abandon open files. |
| Rerror names are lost | `libs/rt/src/client.rs::ClientError::Remote` and `libs/client/src/error.rs::Error::Rerror` retain no name. Implement the shared wire table and preserve names through synchronous and multiplexed completion paths. |
| Path/whole-file helpers missing | `Namespace` only binds and looks up; `File` exposes one-request reads/writes. Implement the native.md path operations and bounded whole reads/writes, including partial-write reporting. |
| VM files are synchronous | `vm/src/platform.rs::Files` returns immediate results; `bif/file.rs` calls it directly. Hub submission alone does not suspend only the calling Erlang process. A completion/continuation design is required. |
| Metadata cannot be honest yet | `FileInfo` requires integer mode/uid/gid/links/inode, and `bif/file.rs` emits them plus device zeroes. Redoubt needs absence represented as `undefined`, preserving real host values. |
| Console still blocks | Redoubt `console_write` calls `write_all`; `console_size` calls the server synchronously. Existing asynchronous input does not satisfy the planned native-call contract. |
| Separate-workspace host tests lack bench routing | `HostTests` accepts packages/tests/miri, but no workspace or features; `beamlet-vm` and `beamlet-redoubt` are in `userland/otp`, and the real platform's host fixture uses `fake`. Add bounded routing for the prerequisite tests rather than running tests outside the bench. |

The original thread split in beamlet.md (two schedulers, four short-call threads, three
reserved waiters, 246 user waiters) must not be implemented literally. M1 has one VM
scheduler. Accepted AIO1 runs the hub on that scheduler, with a completion waiter per
connection and a shared local inbox; `.wash/local/AIO1-implementer.md`, ruled completion
side and waiter clauses, explicitly delegates this page rewrite to BEAM3. `Hub` is Send,
not Sync; it owns buffers by value and admits submissions inline with a 1 ms timeout.
Connection waiters, pending tags, pages and remaining blocking typed calls must be budgeted
separately. No notification object, endpoint-set wait, extra scheduler or kernel change here.

The client prerequisites are specified in native.md's “Dropped files, error names and
generated calls”, not optional conveniences. The M1 plan also includes generated Elixir
clients before beamlet files; that deliverable remains open and needs its own bounded
package or an explicit sequencing ruling. Do not silently delete it from the prerequisites.

files.md still lists the error vocabulary as Open, but native.md and wire.md's “Error names”
already specify the names and their POSIX mappings with Open: none. Recommend reconciling
that stale Open item to those rules, not reopening the vocabulary. A complete adapter must
also extend `FileError` for table entries it cannot represent today, such as estale.

## First package: terminal lookup refusal

Use a small explicit lookup result (`Found(Vec<u8>)`, `Absent`, `Refused`), shared by the
two Platform methods. The default application result is Absent. Keep the Redoubt source's
existing diagnostic reason and single diagnostic at refusal; the VM only needs the terminal
outcome. Do not carry server error strings into new atoms or paths.

`System::locate_module` returns a platform result on Found, searches the code path only
on Absent, and returns no module immediately on Refused. The refusal ends that lookup;
it does not add a persistent name blacklist. Keep bundle-first precedence, lexical/root
confinement and explicit `code:load_binary/3` semantics. R75 verifies system lookup bytes;
it does not forbid the VM loading bytecode it already holds within its granted authority.

Adapt `.app` lookup and all direct callers exhaustively. `beamlet:app_spec/1` currently
returns a binary or `error`, and `application.erl` does no file fallback on that error:
keep that Erlang result contract. A refused app is terminal and must not trigger another
source; absence can remain the existing error here. `module_file` is a path-description
method, not a byte lookup, and need not become a new lookup interface. Logger presence
checks count only Found. Host CLI adapters preserve their existing search policy.

### Verified implementation and caller inventory

A repository-wide Rust search found these six implementations of beamlet's Platform;
every required adaptation is inside the owned paths below:

| Implementation | Methods to adapt |
| --- | --- |
| `redoubt/src/lib.rs::Redoubt` | load_module and load_app, plus their shared private load helper |
| `cli/src/main.rs::Posix` | load_module and load_app; map the existing host lookup result without changing its search policy |
| `vm/src/platform.rs::Clock` | load_module; inherits the new Absent default for load_app |
| `vm/src/vm.rs::Bundle` | load_module and test-only refusal injection; default application behavior unless exercised by an app test |
| `vm/tests/limits.rs::TestPlatform` | load_module; inherit load_app default |
| `vm/tests/hostile.rs::TestPlatform` | load_module; inherit load_app default |

Production direct callers are `vm/src/vm.rs`'s logger/logger_sup presence check and
`System::locate_module`, and `vm/src/bif/info.rs::app_spec`. Adapt each by explicit matching;
do not convert Refused back to an Option before the fallback decision. `application.erl`
consumes app_spec's existing binary/error result and needs no change. Redoubt's
`userland.rs::Verified` and `fixture.rs::Dirs` implement the separate Modules source trait,
already carrying Unloaded::Absent/Refused; their signatures do not change. Other Rust
implementations named Platform in sshd belong to a different trait and are not owned.

### Exact owned paths

- `userland/otp/vm/src/platform.rs`: result type, two method signatures/defaults, local test
  platform implementation and boundary comments.
- `userland/otp/vm/src/vm.rs`: locate_module, logger presence check, existing bundle/path
  unit-test fixtures and new refusal tests.
- `userland/otp/vm/src/bif/info.rs`: app_spec's exhaustive result match.
- `userland/otp/redoubt/src/lib.rs`: preserve Absent versus Refused from Modules through
  both Platform methods; retain diagnostics.
- `userland/otp/cli/src/main.rs`: mechanical adaptation of the two host lookup methods.
- `userland/otp/vm/tests/limits.rs` and `hostile.rs`: mechanical fixture adaptations.
- `userland/otp/redoubt/tests/userland.rs`: extend module/app verification tests through
  the Platform outcome where possible. A focused `tests/lookup.rs` beside it is allowed
  if constructing the real adapter is clearer there. Prefer an in-module test in lib.rs
  when its existing private load helper is the narrowest boundary under test.
- `tools/testbench/src/case.rs`, `build.rs`: only an optional host-test workspace field,
  defaulting to root, and an optional feature list defaulting to empty. Command-building
  tests prove current behavior, routing to `userland/otp`, and forwarding the requested
  `beamlet-redoubt/fake` feature. No generic command runner, scripts, external paths or
  fake successful exit.
- New `tests/beamlet-lookup-host.toml`: host-tests in workspace `userland/otp`, packages
  `beamlet-vm` and `beamlet-redoubt`, feature `beamlet-redoubt/fake`, using the existing
  fake-console/platform fixture to check actual refusal propagation and diagnostics.
  New `tests/beamlet-lookup-cli-host.toml` runs package `beamlet` separately. The CLI's
  default `threads` feature enables VM std; keep it out of the first case so that the
  one-scheduler VM configuration is also tested. No broader feature matrix is required.
- Owning-page updates listed below, coordinated with the Architect in the package worktree.

Not owned: `kernel/**`, scheduler code/tests, `libs/client/**`, `libs/rt/**`, the hub or
wire protocol, server implementations, VM Files/FileInfo APIs, interpreter scheduling,
console threading, `application.erl`, OTP bytecode blobs or generated Elixir clients.
An unexpected caller outside these paths is reported before adding another subsystem.

### Attack tests and acceptance

Use the inventory above and state the chosen result spelling and fixture in the initial
implementation report. R75 and the todo settle the behavior; no further generic design gate
is needed. Escalate an actual contradiction or scope change. Run all tests through
`cargo testbench` in the existing
dev image, using `.wash/local/in-dev`. No installs and no direct cargo-test workaround.

1. A VM Platform returns Refused for a module whose bytes exist in the first code-path
   directory. `locate_module` returns none. Instrument the test Files adapter to count or
   reject access: assert **zero file operations**, not just that the eventual load failed.
2. The corresponding Absent result finds that same path file, with observed backend open/read
   attempts greater than zero; Found wins even over a
   prepended directory. These positive controls prevent an implementation that blocks all
   fallback from passing the refusal test. Existing bundle/path ordering tests stay.
3. The real Redoubt adapter maps matching indexed module/app bytes to Found, an absent
   index name to Absent without an object read, and hash mismatch, missing indexed object,
   short or oversized contents to Refused. Retain the existing verification tests; cover
   propagation past the helper that currently collapses the two failures.
4. Both app and module Platform methods expose all three outcomes distinctly. For apps,
   verify Absent performs zero object reads and Refused performs only the indexed-source
   attempt, never an alternative lookup. Through app_spec, both non-found outcomes return
   the existing Erlang `error` result without a second source read; Found returns its bytes.
   Observe source-call counters and any attempted file operations rather than inferring
   this from the returned atom. Logger presence and startup retain their behavior.
5. Record one negative control restoring refusal-as-absence in a temporary test run:
   the zero-file-access regression must fail. Restore the clean final tree before gates.
6. Both new lookup host cases pass and actually execute the named VM refusal
   test (report test names/counts). Bench command tests prove the default workspace still
   works and a missing/invalid requested workspace fails; never silently run at root.
7. Existing `beamlet-boot`, `beamlet-console`, `userland-boot`, `userland-bad-start`,
   `userland-read-only`, beamlet heap/budget flood cases pass on each declared width.
   Tier A requires the whole bench, rv32 compilation, docs, formatting and size/unsafe
   checks at the final rebased head. Record commands/exits and any unrun check; an unrun
   required check is not acceptance. Review the exact final head and state what was deleted.

This package is Tier A despite most of the interpreter being Tier B: it changes the
capability-facing Platform contract and implements R75's verified-source boundary.

## Owning-page and summary updates for the first package

- `docs/userland/beamlet.md`, Platform boundary: state the three outcomes and allow fallback
  only for absence; name the new host attack in the status. Correct the stale “only the
  host embedding exists” claim using the existing Redoubt implementation and boot evidence.
  Leave asynchronous files/console sections planned; this package does not build them.
- `docs/kernel/boot.md`, R75: retain the no-fallback rule, add the new propagation attack to
  its test/status evidence, without implying that native program verification is built.
- Delete `docs/todo/beamlet-refused-module-falls-through.md` only once its Done when is met;
  remove its `docs/SUMMARY.md` entry and any other direct references found by exact search.
- `docs/testbench.md`: document host-tests' optional workspace and feature list, their
  defaults and failure behavior alongside its existing fields.
- `docs/plan/m1-separation.md`: retain the VM/client remaining-work statements; describe the
  closed lookup prerequisite only if needed to keep those statements accurate. Do not claim
  file I/O or deferred clunk is built. Wash node changes are the orchestrator's; leave BEAM3
  pending and attach this brief rather than pretending its broad acceptance is satisfied.

## Subsequent checkpoints; no broad implementation yet

Keep these as separately reviewable followups, with explicit ordering. The descriptive names
are proposed package scopes, not new plan IDs; the orchestrator assigns IDs and edges.

| Followup | Required predecessors | Completion needed before |
| --- | --- | --- |
| Client fid lifecycle, including in-flight AIO references | API1, ABI2 and AIO1 (done) | Path/whole-file helpers and BEAM3 files |
| Shared named 9P errors | API1 and AIO1 (done); same names on synchronous and hub paths | Path/whole-file helpers, generated clients and BEAM3 file error adapter |
| Path calls and bounded whole reads/writes | Client fid lifecycle and named errors | BEAM3 file adapter |
| Generated Elixir typed clients | Shared error-name table and existing wire codecs | BEAM3 as presently required by the M1 prerequisite list; changing that ordering needs an explicit ruling |
| VM hub/completion integration and files | BEAM7, AIO1, the client followups above, and FSD3 (done for real-volume acceptance) | BEAM3 acceptance, then BEAM4 and BEAM5 |

The lookup fix does not depend on these client followups. They can be prepared while BEAM7
is reviewed; serialize shared client/wire edits by package rather than combining all scopes.

After lookup, use separate bounded client and VM integration packages. Client work must
prove the named deferred-clunk attacks: 1,000 dropped files on one connection without
NoFid; an in-flight read retains its fid; exactly one clunk after last use; a stalled clunk
costs at most one configured timeout to a later call and cannot fail that call; no fid reuse
before a conclusive reply. Reconcile that synchronous rule with hub in-flight references,
flushes and disconnection before exposing files to Erlang processes. Raw-fid AIO requests
must not bypass File lifetime. Named errors require wire-table drift tests and an unknown
text -> other test, without exposing arbitrary remote text. Path and whole-file helpers
need root/bind confinement, maximum-read and partial-write tests. Exact client paths and
cross-crate error propagation get their own brief before assignment.

The subsequent VM integration checkpoint must specify request IDs/owners, bounded queues,
completion routing to the requesting Erlang process, process-exit cancellation and buffer/fid
retention, idle/timer/completion ordering, connection-waiter resource admission, and how
short typed console calls finish away from the VM scheduler. Preserve fresh console-size
queries. Report the quantum cost of inline AIO submits; “asynchronous” does not mean they
consume zero time. AIO1 permits one-connection direct waiting only when nothing else must
wake the caller; a VM with timers, console and file connections does not meet that shortcut.

Before files can ship, test real /boot and fsd-volume reads/writes over the namespace,
directory enumeration, positions and partial writes; killed Erlang owners with I/O in
flight; hostile/malformed/late/duplicate completions; resource saturation with other Erlang
processes and session input/timers still progressing. Prove absent metadata is undefined
and chmod/chown/link operations return enotsup, with actual host metadata unaffected.
Keep AIO1's buffer return once, tag/flush discipline and one-page MAX_WRITE split; 64 KiB
msize is a ceiling, not permission to send a 64 KiB multiplexed write.

These later work items require concrete capacity and lifetime decisions before code; none
is delegated to an implementer to invent. The accepted AIO1 shape and existing wire error
table settle architecture direction. The remaining resource partition and VM continuation
design need an Architect checkpoint; a change to an owner-approved guarantee or a genuine
resource-policy tradeoff goes to the owner with a recommendation. No owner question is
needed to launch only the terminal-refusal prerequisite under the existing rule.
