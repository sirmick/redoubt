K20 is COMPLETE: wp-k20 commit 2de60cc58 on 31dfad3f0, clean tree, reported, awaiting review. Full handoff: /home/mcloonan/redoubt/.wash/local/K20-handoff.md.

Done:
- billing fix: timer tail to the last expired item's or stale wait's budget; leave bills the payer through to the return
- arming: settle's block branch arms, mark does not
- next_timeout stale-wait payer
- trace I/B/E/O + oracle check + unit test; timer-tail-billed negative fails on both widths
- cancelled-waits case (15 ms, 2 waiters, a departure from the ruling's few hundred µs)
- page lines per K20-empty-timer-ruling.md, plus related page updates
- size ceiling 7856

Numbers:
- empty walks 5,236 -> 0
- shares rv64 494/494/493, rv32 492/492/492
- whole bench 280 PASS + 1 SKIP; size-budget passes after the raise

Traps:
- K16 next_timeout loop-header hunk and leave() overlap; whoever merges second rebases
- no `as u8` for a PID
- use in-dev and nightly rustfmt
