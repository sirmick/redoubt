# K18 early checkpoint: the two-lease run before any kernel change (seed 4, 2026-10-02)

Scratch branch `k18-scratch-before` = main 307b5d818 + the GATE1 fixture (d7d1ab92d, 555aeb6a5,
e9ecc5ea5 cherry-picked cleanly). Not on wp-k18; wp-k18 is still at 307b5d818, untouched.

Command, from the k18 worktree:
`.wash/local/in-dev cargo testbench kernel-containment` (both widths), exit 1.
Output: K18-before-seed4.out. Consoles: K18-before-rv64.console, K18-before-rv32.console.
Split: K18-before-split.txt (GATE1-two-lease-split.py with the console path changed; a reading only).

## Post-check

| | rv64 | rv32 |
| --- | --- | --- |
| deadline notice net p50/p99/max µs | 22877/44343/44343 MISSED (40000) | 23254/45290/45290 MISSED |
| gross p50/p99/max | 81780/114124/114124 | 84154/116777/116777 |
| R10 p50/p99/max | 18684/22124/22124 met | 18749/22278/22278 met |
| lease end | 7936 + 22124 = 30060 met | 8337 + 22278 = 30615 met |
| audits | 230, 2563770 µs | 230, 2489117 µs |

Every other target met on both widths (see the .out).

## Split, net µs (p50/p99 over the 9 D leases)

| term | rv64 | rv32 |
| --- | --- | --- |
| a, expiry | 283/385 | 314/358 |
| b, the walks | 21854/22124 | 22087/22278 |
| c1 | 527/566 | 684/732 |
| c2 | 11379/21676 | 11815/22297 |

c2 is bimodal: about 11 ms (one whole slice) or about 22 ms (two). Every lease shows the GATE1
pattern: K42 takes receipt 1, the U2/V2 audit (about 10.8 ms) runs in its slice, R42 (requeued,
never D42), then the victim K43 (and in the 22 ms leases an H budget too) runs a slice at the
floor, then K42 takes receipt 2. Worst lease rv64: 283 + 21966 + 418 + 21676 = 44343.

Against GATE1's rv64 run on its own base (57053): b fell from about 25.2 to 21.9 ms (main's
K15/K17 work), c2 lost one slice at p99 (31.3 -> 21.7 ms). The miss is still entirely c2's
audit-made requeue, which is what K18 removes.
