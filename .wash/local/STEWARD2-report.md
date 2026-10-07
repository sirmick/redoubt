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

### Final state (after the q/B19 resume), head d930a7409
14 logical commits on bd6f768f6, no WIP or fixups. Not rebased onto main (FSN1 littlefsd, B19):
waits for the orchestrator's word. Local ref s2-wip-backup (old WIP head) may be deleted.

Runner: scripts/jobs.mk now passes --exact/--prebuilt, which this pre-B19 branch's testbench lacks
(prebuilt rc=2), so cases ran as `scripts/q run --cores 1 --lock net -- cargo testbench --arch W
case` (boot cases the same without the lock). Real-time QEMU (no icount on this base), beside
other members' work.

Cases, exit 0 unless noted: steward-ssh-two-principals rv64 0 (709.9 s, memory scan) rv32 0
(909.2 s); steward-vault-session rv64 0 (863.1 s) rv32 0 (907.3 s); steward-sub-budget-flood rv64
0 (935.1 s) rv32 0 (1076.8 s); steward-login-refused rv64 0 rv32 0; steward-session-ends rv64 0
(673.1 s, memory scan) rv32 0; steward-restart rv64 0 rv32 0; steward-boot, init-boot,
userland-boot, userland-read-only, userland-bad-start, verity-flipped-tree, verity-wrong-root
both widths 0; elixir-oracles 0; steward/init/sshd/wire host-tests cases 0; docs 0.
Short gate at d930a7409: doccheck 0, cargo +nightly fmt --all --check 0, unsafe-budget 0,
size-budget 0. Host tests (cargo test -p): redoubt-steward-server 19, redoubt-steward 36,
redoubt-init 71, redoubt-sshd 25, redoubt-sha256 2, testbench 101, redoubt-rt all: 0 failed.
Not run: the whole bench (the train's).

Case 4, re-aimed: one flooding evaluation in the vault session meets beamlet's process limit (a
sixteenth of the budget) and is killed (`:killed`), the session answers on (1001), bob and
alice's unlabelled session print 55. The kernel's refusal of the sub-budget is reachable only by
several flooders at once, which hits the VM's backstop and ends the VM (beamlet-budget-flood's
case), so the brief's "kernel's refusal as the VM reports it" is not this case's verdict. Logins
are sequenced: the console session plus three logins booting VMs at once on one hart reached no
prompt in 700 s (probe; finding for the orchestrator: idle and booting VMs share one hart badly).

Login latency (steward-ssh-two-principals, release, one hart, shared host; alice / bob beside
her): ssh start to the VM's first line 3.4 / 5.3 s rv64, 6.2 / 7.6 s rv32; to the prompt 281 /
235 s rv64, 336 / 327 s rv32. Key exchange, login and the steward's batch are the first seconds;
the rest is the shell starting in the VM. In sshd.md "Sessions over SSH".

K23 (owner): replaces steward-restart's reboot with a restart that logs every session out; the
case and the page residual stay as today until then.

## steward2-implementer-4 (2026-10-07): rebased onto b60c7cc5c, gate, head 10bda633a

### Branch
wp-STEWARD2 at 10bda633a: 15 logical commits on main b60c7cc5c, no fixups, nothing pushed. Rebase hunk logs (second to fifth rebase): .wash/local/STEWARD2-rebase.md. range-diff s2-pre-rebase5 (on f820b6ba3) -> head: commits 1-14 "=" except consol (+ ended is a `send`, regenerated) and wire (+ the generated Elixir client steward.ex), image "!" (m1-separation, shell.md status, size ceilings; the pages and messages below).

### What changed since the handoff
- sshd: ipd's wait ending (`Rerror(Timeout)`, main's one error table) no longer ends the accept loop or a reader: `servers/sshd/src/listener.rs` asks again; NOT_STARTED renumbered 4 (3 is rt's RECEIVE_FAILED). Host test `host:redoubt-sshd::an_accept_ipd_s_wait_ran_out_on_is_asked_again` against the real ipd on the fake kernel; with the old `Remote` match it fails (the listener stops asking). sshd.md Listening paragraph + status.
- beamlet (own commit, orchestrator's (a)): `limits()` divides by the VM's 8-byte word (`VM_WORD_BYTES`), not usize: rv32's process limit was twice rv64's in bytes, so the list flood's heap doubled to 651,312 terms (10,420,992 bytes) before the kill and the VM died. Host test limits.rs; beamlet.md sentence. The flood case now passes rv32 most runs (see open risks).
- consol `ended` declared a `send` in its table (it is sent one way; consoled reads it in its send hook), regenerated: rust `is_send`, Elixir client sends it. steward.ex generated for main's Elixir client generator. libs/client/tests/console.rs (BEAM3's) answers `ended` as refused.
- boot-profile targets by the rule as written: verified 20 s (15.4 s measured rv64), unverified 15 s (13.2 s); rv32 not measured (blocked, below). Breakdown on beamlet.md (0.6-3.1 s init pushes beamlet's 4 MB entry; 3.1-7.7 s carve + streamed launch + pack read; 7.7-15.4 s the shell; unverified 3.1/5.6/13.2 s) and a table row.
- sshd.md latency: a login reaches its prompt 2 to 4 s after ssh starts (first line 1.1/1.1 s rv64, 1.8/2.0 s rv32; prompt 2.1/2.1 s rv64, 3.9/4.0 s rv32, alice/bob).
- budgets.md: the image paragraph rewritten for the steward image (bound 550 pages both widths; servers 16,909 system pages; sessions carved under users; 1 GiB); the steward-carving status gap closed (+ bench:steward-boot); residual + todo/empty-a-budget.md updated for budget_reap (the call exists; init's use remains, K23's).
- Pages: init.md duplicate `console` row merged; steward.md and sessions.md package IDs removed; shell.md "The shell in a session" status; README/m1 summaries merged with BEAM3/BEAM4/WFS2.
- Commit messages: bootfsd -> "bootfsd, erofsd: client budgets with room for every session's domain"; image commit rewritten (names, sizes 43,528 / 10,881, buckets, peaks, latency, targets).

### Gate at 10bda633a (jobs.mk, one case each; exit 0 = PASS)
PASS both widths: steward-boot, steward-restart, steward-login-refused, steward-session-ends, steward-vault-session, steward-ssh-two-principals, init-boot, bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page (smp 1 and 4), image-disk, beamlet-heap-flood. PASS rv64: steward-sub-budget-flood, userland-boot, userland-read-only, boot-profile (20 s), boot-profile-unverified (15 s), elixir-oracles, steward/init/sshd/wire/client/r4 host-tests, host-tests, docs, formatting, unsafe-budget, size-budget.
At 28cfb0c0a (same commits before the wire regen and page edits), additionally PASS both widths: userland-bad-start, verity-flipped-tree, verity-wrong-root, verity-signed, verity-bad-signature, verity-rollback, beamlet-footprint, beamlet-budget-flood; rv64 rt-host-tests; and the rv32 flood PASSED there.
Host: cargo test -p redoubt-sha256 (2), beamlet-redoubt --features fake (38), redoubt-init, redoubt-wire(-gen), redoubt-client, redoubt-sshd: 0 failed.
FAIL:
- rv32 userland-boot, userland-read-only, boot-profile, boot-profile-unverified: BLOCKED on BEAM9 (merging): the console session's writes refused busy behind a parked read pinning consoled's one-page share (rt ninep_mux stored()). Probe write-up .wash/local/STEWARD2-rv32-probe.md. To rerun after the rebase over BEAM9, plus the rv32 boot-profile figures for beamlet.md's row.
- rv32 steward-sub-budget-flood: intermittent (passed at 28cfb0c0a, failed at f820b6ba3-base and at 10bda633a): bob's VM, idle at its prompt, is terminated (kernel "terminate_process", no fault line) right after alice's vault login, during or just before her flood; the steward then reports users/bob/{} empty and sshd ends bob's channel. The orchestrator links the symptom to K27 (a refused carve ending the VM silently). Not diagnosed further; it is this case's own R37 verdict on rv32, so it stays open until K27 or a diagnosis.
Not run: the whole bench (the train's).

### Summaries checked
README.md "Today" (updated: steward and SSH sessions; native launching, leases, agents remain), docs/plan/m1-separation.md (VM bullet, steward bullet, built list, not built), docs/servers/README.md (holdings row, graph), docs/servers/init.md (steps 5-6, table), docs/kernel/budgets.md (image paragraph, status, residuals), docs/userland/shell.md and sessions.md (statuses), docs/servers/steward.md, sshd.md, beamlet.md, image/README.md, image/disk.toml header, docs/testbench.md memory table. GETTING-STARTED.md: no steward/sshd/1 GiB claims found to change (checked by grep).
