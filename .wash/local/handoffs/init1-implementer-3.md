INIT1 parked at wp-init1 tip d0e385529 (base main 908fb83c4; main is now 15b510297, rebase before folding). Full detail: /home/mcloonan/redoubt/.wash/local/INIT1-progress.md, section "Handoff from init1-implementer-3".

Commits:
- d8a6330af, c31997716: D1, accepted.
- 7bcdb9998: model WIP, lands last.
- 0c032ae20: D2 WIP.
- 58996a42c..235f0f179: D3 WIP, nine commits. Own budget in slot 3 and per-program exit endpoints. The 8 attack programs and the victim are reworked. The 17 cases (and 4 more) are moved or fixed; the table is in the file.
- d0e385529: pages and the tester's named exit lines. Also D4's bundle-file.rs, NOT built or run yet.

Verified per filter, both widths: all budget cases, the moved cases, rng, dma/device/ipc, sched-latency. sched-latency's fix carves a measurer budget under system; it passes but with less p99 margin than main.

Left:
- D4: Cargo.toml bin entry; the case toml is staged at .wash/local/INIT1-bench-bundle-file.toml.
- Run budget-destroy-kills, rng, boot-stack-reservation and bench-bundle-file.
- Whole bench, both widths, with `--allow-skip`.
- Gates: fmt, doccheck, size and unsafe.
- Fold: D1 x2, then D2+D3+D4 as one commit, then the model.

Rulings: 8074ecf59, 4ace1722d, 908fb83c4.

Traps:
- Run everything via in-dev.
- Don't edit sources mid-bench.
- A started program has no devices and no untouched pages.
- system has 0 free pages after the shares.
