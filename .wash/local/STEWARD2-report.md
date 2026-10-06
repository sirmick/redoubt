# STEWARD2 report (steward2-implementer)

## Early checkpoint (2026-10-05), branch wp-STEWARD2 at main 6d0ce2090, nothing committed yet

### First-step edit list (point 1: lines, init producing them, steward carving)

- `libs/steward/src/manifest.rs`: a strict no_std parser, `Manifest::from_lines(&[&str]) ->
  Result<Manifest, LineError>` (the line number and why), for `principal`, `keyd`, `servers`,
  `sizes`; its lexical helpers (tokens, `[..]` lists, `[[..],..]`, `P,N,W`, quoted strings with
  `\" \\ \n \xNN`) move from `trace/src/text.rs`. Strict where the trace parser is lenient today:
  `servers` > u16 refused (now truncates), a missing or repeated `sizes`/`keyd`/`servers`
  refused, unknown fields refused, an empty `servers` token refused.
- `libs/steward/src/hash.rs`: `key_id(&[u8; 32]) -> u64` (first 8 bytes LE of SHA-256), with a
  vector test.
- `libs/steward/trace/src/{input,text}.rs`: manifest lines go through the core's parser; event
  lines stay here. Output byte-identical; `elixir-oracles` must stay green.
- `libs/steward/fuzz/` (or `tests/`): a fuzz target over the line parser; host tests.
- `servers/init/src/manifest.rs`: `steward { sizes { session, agent, sub_agent, crossing, cost } }`
  and `console` (a principal name), both optional.
- `servers/init/src/check.rs`: `STEWARD` program const; checks (sizes nonzero; under every
  principal's smallest sub-budget; `console` names a principal; a `steward` server needs the
  `steward` object); `lines(m) -> Vec<String>` (each principal with `sets=` its unlabelled set
  first, then each distinct set, as `domains()` counts them; key ids by `key_id`); `args()` appends
  the lines for the steward; the `BUDGETS` comment's R33 exception (the steward is handed `users`).
- `servers/init/src/bin/init.rs`: `start()` places `users` by name for the steward only.
- `servers/init/tests/manifest.rs`: the new checks and the lines.
- `servers/steward/` (new): `Cargo.toml`, `src/lib.rs` (args -> Manifest -> `Store::boot`, carve
  runner over a `Kernel` trait so host tests run it), `src/bin/steward.rs`; workspace member.
- `image/manifest.json`, `image/boot.toml`: labels `alice-secrets`, principals alice and bob, the
  steward entry, `steward.sizes`; `tests/keys/`: alice's and bob's test keys.
- `tests/init-servers.toml`: the budget tree lines.

### Contradictions and gaps found

See the message to the orchestrator (Q1 to Q3, notes N1 to N3).

## Brief checkpoint (point 1), wp-STEWARD2 at 1be6b8686 (2 commits on main 6d0ce2090)

Rulings applied (QA STEWARD2-manifest-lines): Q1 (a), Q2 (a), Q3 SLOTS = 5, N3 `steward.server`.

Commits:
- 752d09fb4 steward: the core reads its manifest lines, and init writes them. Core: strict no_std
  parser + writer in `libs/steward/src/manifest.rs` (trace crate uses both), `hash::key_id`,
  fuzz target `libs/steward/fuzz` (+ kept corpus, `fuzz` feature), `tests/lines.rs`. init:
  `steward {server, sizes}` and `console`; checks (Unknown on steward.server/console, Budget on
  sizes, new `Why::Sizes` against each principal's equal share, Argument on any steward-entry arg
  but buckets=); `steward_lines` appended by `args()`; `users` handed by name to that entry only
  (`USERS`, `is_steward`); `label_sets[].budget` removed (fixtures and 2 named seeds updated);
  init now depends on `redoubt-steward` (brings sha2: TENETS 5 note below). Pages: init.md table
  rows, step 6, status list; steward.md "The manifest lines" (new ####), trace encoding points to
  it, equal-share sentence + M5 line, keyd residual.
- 1be6b8686 steward: the server starts from its lines and carves the principals.
  `servers/steward` (`redoubt-steward-server`, bin `steward`): `start()` behind a `Kernel` trait,
  `SLOTS = 5`, start failures said once. Case `tests/steward-boot.toml` (own manifest, no net).
  `image/boot.toml` packs `steward`; no manifest starts it yet.

Commands (all through the jobserver), exit codes:
- `cargo test -p redoubt-steward -p redoubt-steward-trace -p redoubt-steward-server -p redoubt-init`: 0
  (core 30 + lines 6, trace 6, steward 6, init lib 15 + manifest 46 + bound 1)
- `make ... rv64/steward-boot rv32/steward-boot`: 0 (PASS both)
- `make ... rv64/elixir-oracles rv64/bench-elixir-oracles-broken-guard`: 0 (PASS both)
- `cargo test -p redoubt-doccheck --test docs`: 0 (after two fixes: M5's full name; a hex
  digest in a comment read as a commit hash)
- `cargo +nightly fmt --check`: 0; `git diff --check`: clean
- Not run yet: whole bench (image cases now pack the steward), rv32 build of everything, unsafe
  ratchet (the steward adds none: `forbid(unsafe_code)`), size gate.

Size: steward 371,192 bytes rv64 / 334,112 rv32 (release ELF), keyd 221,400 for scale: the core's
state machine, its tables and sha2. Not yet against the size budget.

Departure: `init-servers` is not extended. The bench's `expect` is ordered, and the steward's
lines race boot-reader's verdict there; the budget tree has its own case, `steward-boot`.

Risks / notes:
- TENETS 5: init's Cargo comment said "only our own crates"; init now links sha2 through the core
  (the brief puts key_id in libs/steward). Alternative: keyd's hand-written sha256 as a shared lib.
- A restarted steward carves again under `users` while its first carves remain (they hang off
  `users`, not the steward's budget); a carve failing part-way leaves earlier ones. steward.md
  "Failure and restart" territory; not handled.
- A key in two roles across principals is refused by the core at the steward's start, not by
  init's check: a start failure, said once, rather than a boot refusal.
- The steward entry's `buckets=` is not yet required (admission is point 6).

## steward2-implementer-3 (2026-10-06): blocker fixed, cases, peaks, fold

Branch wp-STEWARD2, 14 logical commits on main bd6f768f6, no WIP (backup ref s2-wip-backup, local
only). Not rebased onto FSN1 (littlefsd): waits for the orchestrator's word.

### Commits
35f64bb49 core manifest lines · 007f20919 steward server starts and carves (+ unsafe budget
redoubt-steward-server 0) · 1bf12a957 sha256 (+ R45 path in SECURITY, unsafe budget
redoubt-sha256 0) · 251724ae3 init key once · ec1ad7fda Console row · ba927cec9 wire steward
table · 53a0a4a86 client streamed launch (its tests folded in) · 426a43b67 consol ended ·
0e83304db rt mint at own root (R25 ruling) · e045d041d bootfsd 1 MiB · 5b1c3cb14 testbench
expect_after, leading waits, marks after exit · 5aefbfce5 steward login/batch/console (WIP
fixes folded) · 3bcde4481 sshd box platform (Unsafe budget line) · image commit (Size budget
lines for client, rt, wire, steward core, sha256, sshd, steward server, consoled, init).

### The blocker and what followed (all fixed, each with its test)
1. sshd's per-channel skeleton had buckets 1; Admission refuses caps below 2, so every driver
   returned silently before its ident. Now LIMITS in console.rs (2 buckets), host test
   a_channel_s_skeleton_is_sized_as_admission_allows; the silent path says a line.
2. Driver stack 16 pages overflowed in the key exchange (PC/RA 0x88b8 = BUF 35,000 written over
   a return slot; slot stacks have no guard). Host probe: a login (client+server, one thread,
   x86_64 release) fails at 88 KiB, fits 96 KiB; driver stack now 48 pages.
3. sshd's lines lost: every thread says through one console connection whose attach resets the
   fids; say() now holds a lock. Case 5's BadKey line is the test.
4. Vault batch: CreateScope made an unlabelled child of a labelled budget (LabelDenied): the scope
   now carries the session's labels (host test asserts it). Then the steward attached to the
   labelled volume before minting (R25 refused): it now mints and disconnects unattached; and
   new_connection's root check: the Architect's R25 ruling (a), in libs/rt (owned by the ruling).
5. Ordering: sessions started their ssh at once whatever the waits; the steward said its lines
   after answering. Now a session's leading waits come before its ssh, marks may follow exit, and
   the steward says a call's records before answering, so expect_after chains are causal.
6. Failed batch steps are said with the kernel's error (verity cases now verify the steward
   refusing the console session on a corrupt system volume).

### Owned-path extensions to record
servers/bootfsd + bootfsd.md; tools/testbench/src/{case,qemu,ssh}.rs + testbench.md (ssh.rs is
new here: leading waits); libs/client launch.rs and console.rs; libs/rt ninep_mux.rs (Around::send)
and ninep.rs (R25 ruling); consoled (ended); README.md and docs/plan/m1-separation.md summaries.

### Measurements
Alice top 98,312 pages (two 24,577-page sessions per label set). Peaks (stack bytes / heap pages),
largest over init-boot, userland-boot, userland-read-only, steward-session-ends (both widths) and
steward-ssh-two-principals rv64: keyd 6,592/4, consoled 9,144/10, bootfsd 7,320/1,540, blkd
4,520/19, netd 4,296/2, ipd 13,672/37, fsd:data 10,312/10, fsd:alice-secrets 7,192/9, blkd:system
4,520/17, verity:system 7,864/47, fsd:system 12,696/20, steward 13,224/14, sshd 5,256/54 (cap 384 =
2 x (8 + 4 slots x 46)). Declared 2x; table in docs/testbench.md "The memory budget".
steward-restart's steward heap cap is 9 (carve <= 9 pages both widths; launch 10 rv32, 12 rv64).

### Cases 2 and 3 (ruling (a))
Verdicts are the steward's records and the kernel's usage through them, sshd's channel labels,
markers kept per channel; file verdicts move to BEAM3. Case 5's session-badge login is host-only
(every_operation_on_another_badge_class_is_malformed). steward-restart cannot show the console
session back after the reboot (the heap provocation repeats); it asserts the reboot's fresh carve.

### Summaries checked
README.md (Today, M1 line: updated), docs/plan/m1-separation.md (built list, Not built, remaining
work: updated), GETTING-STARTED.md (no claim), image/README.md (updated), servers/README.md (row
and edges; section status restored to planned: its device-handle Open item is not ours),
sessions.md (statuses partly tested, naming host tests), sshd.md (Open questions moved to
residuals), steward.md, init.md, SECURITY.md (R25, R33, R36, R37, R45, R67), todo pages.
