# B111 report: nscr-interrupt rv64 hang under load

Branch wp-B111, base 78664b727 (main), head b774d95c6 (one commit, tests/nscr-interrupt.toml only).

## Repro

Load: rv64 nscr-interrupt x48, 12 at once (q, 1 core each, --share qemu, from target/prebuilt at
78664b727) beside shuffled rv64 boot-class cases 8 at once (q, 2 cores each). Scripts:
.tmp/B111/loop.sh, .tmp/B111/load.sh.

Result: 47/48 pass, 1 FAIL `nscr-interrupt [rv64, smp=1] 450.2s timed out waiting for
/\{:ctrl_c, nil\}$/`, the trains' failure exactly. Console kept:
.tmp/B111/load1/9/rv64-nscr-interrupt.smp1.log. Background load: 438 cases, 0 failed.
Baseline alone: 4/4 pass, 7.6-8.5 s.

## The lost step

From the failing console, after the flood step:

    [con 4d591a58dc674240] {:flood, nil}
    ... /home/alice (6)> dd{:ctrl_c, (up.("up-ctrl-c"); Redoubt.Screen.Native.run("screen-stage", ...
    [con 4d591a58dc674240] error: undefined function dd/1 (there is no such import) (shell:6)

The flood input `ESC[200~ dd ^\ dd ESC[201~` reached the driver in two reads. Ctrl+\ in the first
ended the screen correctly (the driver drops what follows the interrupt in the same read only).
The tail `dd ESC[201~` came in a later read, after the screen had ended, so it was type-ahead
for the next line, which is the shell's correct behaviour: keys typed after an interrupt are the
next line's (docs/userland/shell.md, "What a job cannot do": a stage the interrupt ends never
takes what is typed for the next line; the line does). The case then typed its next line behind
it, the line ran as `dd{:ctrl_c, ...}`, failed, no screen ran, no `up-ctrl-c` tag or `key:ctrl+c`
came, and the case waited 450 s. Not a kernel line, not the interrupt path: the case's own
leftovers. The draw step's `ESC[1;5 ^\ A` carries the same exposure (a split leaves `A` at the
prompt before `{:after_draw ...}`).

My first hypothesis (a kernel `mem:` / `[!] Terminating` line cutting the draw line between
consoled hold chunks) was wrong; the VM serializes both console ports into one queue, and the
failing console shows no split draw.

## Fix

tests/nscr-interrupt.toml: the two lines typed after an input carrying bytes after Ctrl+\
(`{:after_draw ...}` and `{:ctrl_c ...}`) begin with Ctrl+U, edlin's backward_kill_line
(OTP 28 edlin_key.erl `"\^U" => backward_kill_line`; the driver passes 0x15 to group at the
prompt, only 3, 4 and 0x1C are its own). Comments state why the tail is correct type-ahead and the
Ctrl+U the case clearing its own leftovers. What the case judges is unchanged: the bytes after
Ctrl+\ in the same read still reach no program (forbid `key:(A|ctrl\+Up|d)`). No other case sends
bytes after an interrupt in one input (grep tests/*.toml for `\u001c` / `\u0003`).

## Evidence

- Forced split (scratch variant, not committed, worktree removed): each tail sent as its own
  input just before the next line. With Ctrl+U: PASS 8.5 s. Without (timeout 60): FAIL 60.1 s,
  `timed out waiting for /(^|\] )\{:after_draw, true, \{:error, :not_running\}\}$/` (the `A` tail).
  Deterministic proof the Ctrl+U clears the tail and the tail alone breaks the old case.
- `make -f scripts/jobs.mk set CASES=nscr-interrupt` at c495c5eca (same inputs, older comment):
  rv64 PASS 9.2 s rc=0, rv32 PASS 9.8 s rc=0.
- `make -f scripts/jobs.mk prebuilt` at b774d95c6: rc=0. `make -f scripts/jobs.mk docs`: rc=0.
- Witness at b774d95c6, same load as the repro (50 at 12 beside boot cases at 8): **50/50 PASS**,
  0 failures; times min 7.5 s, median 9.7 s, max 11.5 s. Background load during it: 79 cases,
  0 failed. Results: .tmp/B111/witness/results.

- Fold after review (servers-red OK with notes; Architect ruling 'B111 review P2', the driver
  behaviour to B113): the case comments and commit message no longer cite shell.md; they say a
  read arriving after the interrupt is new input for the next line, so the case clears its own
  leftovers with Ctrl+U. Inputs unchanged. Head f1687fbed: prebuilt rc=0, docs rc=0,
  `set CASES=nscr-interrupt` rv64 PASS 7.4 s rc=0, rv32 PASS 7.3 s rc=0. (The witness ran at
  b774d95c6, whose inputs are identical; the fold changes comments only.)

Not run: formatting gate (no Rust/Elixir changed), rv32 under load (the failure was rv64 only;
rv32 passes alone), whole bench.

## Documentation check

- tests/nscr-interrupt.toml description: "the rest of the sequence reaches nothing" still true
  (it reaches no program); unchanged.
- docs/userland/shell.md "What a job cannot do" and docs/plan/m2-usable-shell.md "No program
  swallows the interrupt": behaviour unchanged, no update needed; the case comment cites the page.
- README.md, GETTING-STARTED.md, milestone progress: no claim about this case; no change.

## Risks

None known. Ctrl+U on an empty line is a no-op in edlin.
