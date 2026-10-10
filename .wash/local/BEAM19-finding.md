# BEAM19 finding: beamlet's cost per loop step on the machine (from B46, 2026-10-09)

**Measured:** on rv64, smp 1, `icount = "shift=3"` (sleep on, since the case has a disk), at alice's
console prompt, image/boot.toml at 1 GiB. Times are the session's own `System.monotonic_time` in
guest time. Each line was typed via `[[input]]` and run as `cargo testbench --exact --arch rv64
<case>` through `q run --cores 8`.

| expression, `z = :binary.copy("z", 262144)` | guest time | per element |
| --- | --- | --- |
| `Enum.reduce(1..262144, 0, &+/2)` | 22.7 s | ~87 us |
| `Redoubt.Term.Width.run(z)` as a tail-recursive byte match (B46's early version) | 17.2 s | ~66 us |
| `Redoubt.Term.Text.visible(z, 0)` (B46's early byte loop) | 17.4 s | ~66 us |
| `:binary.match(z, "\e")` | 0.13 s | |
| `:re.run(z, "^[ -~]*")` | 0.15 s | |
| `:binary.copy(z)` | 0.054 s | |
| `String.length(z)` | 3.5 s | |

**Comparison:** host beamlet runs the same loops in about 10 ms. At `shift=3` the guest is a
125-MIPS machine, so 87 us is about 10,000 guest instructions per step. That is far more than
TCG's slowdown, so the suspect is a per-reduction platform cost (a kernel call or clock read per
step?) or another VM thread spinning. Not investigated further.

**Gotchas:**
- A typed line is evaluated by `erl_eval`, so a binary comprehension typed at the prompt is
  interpreted per byte, far slower still (it hung past 300 s). Put loops in compiled code, or use
  `Enum` with external funs (`&+/2`), as above.
- Console input under icount: keep typed lines under about 580 characters, and send later lines
  after a printed line, not a prompt (prompts end without a newline, so `after` never matches).

**Reproducing:** a scratch copy of `tests/shell-output-rate.toml` (wp-B46) whose `[[input]] send` is
`t=fn f->s=System.monotonic_time(1000000);f.();System.monotonic_time(1000000)-s end;z=:binary.copy("z",262144);IO.inspect({:probe,t.(fn->...end),...})`
and whose `expect` is `'^\[con [0-9a-f]{16}\] \{:probe, [0-9]'`.

**Related, also in beamlet:** `:binary.match` (`userland/otp/vm/src/bif/binary.rs`, `find`) is
O(bytes × patterns) and copies the haystack into a `Vec` on every call. `compile_pattern` keeps
only the pattern.

**Floors to re-set:** B46's `shell-output-rate` floors (12,000 B/s for each text, 250 ms for the
redraw) were set on this per-step cost, and are re-set once it falls.
