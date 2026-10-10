# B32: what grew in the shell VM's heap since SHELL2

How it was measured: Redoubt.Shell on the fake kernel (`fake-redoubt report_memory`, userland/shell/setup.sh)
at c144f28fc^1 (before SHELL2) and at d0a19ac6c (main), typing one line that lists
`:code.all_loaded()`. Script: .tmp/B32/loaded.sh; outputs: .tmp/B32/loaded-{B32-pre,B32}.txt.
At the prompt the fake matches the machine's rows to within a few pages (rv64 machine 4,316
accounted pages, fake 4,346).

| | before SHELL2 | main | delta |
| --- | ---: | ---: | ---: |
| modules loaded at the prompt (fake) | 103 | 115 | +12 |
| accounted pages at the prompt (fake) | 3,926 | 4,346 | +420 |
| rv64 machine: accounted at the prompt (beamlet.md table, then B30 run) | 3,897 | 4,316 | +419 |
| rv64 machine: runtime heap peak at the prompt | 4,584 | 4,999 | +415 |
| rv64 machine: the scan's peak after one command | 5,287 | 5,902 | +615 |
| rv32 machine: the scan's peak after one command | 5,108 | 5,703 | +595 |

rv64 rows on the machine, before SHELL2 → main: instructions 518→572, operands 2,095→2,311,
shared literal table 771→857, module tables 231→254, atoms 94→101, process heaps collected
46→56, the rest of the processes 100→121.

Modules loaded since SHELL2 (after one command), with their .beam sizes in bytes. Decoded code is
about 1.4 to 3 times its .beam size.

| module | .beam | why |
| --- | ---: | --- |
| shell | 120,232 | group and edlin call shell:prompt_width/1 and shell:default_multiline_prompt/1 |
| gen_statem | 105,740 | group is a gen_statem |
| prim_tty | 69,012 | group and edlin (character widths) |
| Elixir.Redoubt.Term | 68,060 | the driver's encoder |
| Elixir.Protocol | 66,080 | the shell is built with consolidate_protocols: false; Enum.chunk_while in Term goes through Enumerable |
| group | 62,396 | the line editor's process |
| edlin | 43,956 | line editing |
| sys | 36,840 | gen_statem |
| group_history | 18,004 | group:init calls group_history:load() |
| Elixir.Redoubt.Shell.Driver | 15,852 | the driver |
| kernel | 15,236 | kernel_safe_sup (application.erl), which group waits for |
| edlin_key | 13,488 | key decoding |
| Elixir.Enumerable | 9,384 | as Protocol |
| Elixir.Enumerable.Range | 7,916 | a range enumerated at the prompt |
| total | 632,196 | |

Nothing was unloaded. The peak after one command grew by about 200 pages more than the prompt's
held pages did. That is the first line's work through group/edlin and the redraw, not measured
row by row.

What can be cut without changing behaviour: Protocol, Enumerable and Enumerable.Range (about 83
KB of .beam) by keeping Term off the Enumerable protocol at the prompt. Perhaps 40 to 60 pages. The
rest is OTP's line editor (group, edlin, edlin_key, prim_tty, shell, gen_statem, sys,
group_history, kernel) and the driver and Term themselves.

The rule (docs/kernel/budgets.md): session pages = 2 × the rv64 peak + the 18-page stack, rounded
up to 128. With 5,902 that is 11,822, so 11,904 (heap cap 11,885; 81 pages over twice the peak).
To stay at 11,776 the peak must be ≤ 5,879 (a 23-page cut). To stay at 11,008 it must be ≤ 5,495
(a 407-page cut).
