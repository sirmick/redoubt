# BEAM17 report: persistent_term's word limit, ETS's live sizing of stored resources

Branch wp-BEAM17 (/home/mcloonan/redoubt/.worktrees/BEAM17), on origin/main b7f3cf055:
- 4ae5d8a48 beamlet: persistent_term has a word limit, and ETS and persistent_term count resources live
- 82973d64f beamlet: persistent_term gets a sixteenth of the VM's budget

## What was delivered

persistent_term:
- `Limits::max_persistent_words` (default 2^27 words, as ETS). `System::persist` counts the
  value's term words, and a new key's, and refuses with `system_limit` past it (or past 2^16
  keys). Cumulative: a value replaced or erased stays in its literal chunk, so its words count for
  good; an erased key's own words are released (`System::unpersist`). Literal parts of a value
  are not copied and not counted again. `persistent_term:info()` memory reports the count.
- On Redoubt `limits()` gives it the same sixteenth as heap and ETS.

ETS (option a, incremental):
- `term::Holdings`, one per store (ETS, persistent_term): sum of the declared bytes of the
  resources its terms hold. Each `Resource` keeps `stored[store]`, how many terms of that store
  hold it. Terms enter/leave (insert, remove, remove_object, replace, clear, `Drop for Table`);
  `Ctx::resize_resource` now sets the size under the system lock and calls `System::resized`,
  which adds (new-old) x count to each store. ETS's tables share one `Arc<Lock<Holdings>>`.
- `ets::weigh` counts term words only; `Tables::words` = sum of tables' term words + holdings.
- Cost: insert/delete/replace = one step per resource on the objects added or removed (each
  object's own off-heap table); resize = one step per store (O(1)); deleting a table = one step
  per resource its objects hold. No rescans. Before, `Tables::words` was already O(tables).
- Choice: refused, not killed. A grown stream makes the next insert/put `system_limit`; the
  growth itself is bounded by the grower's heap limit.
- Found and fixed on the way: `room_for` let a zero-word object (a literal) in while ETS was
  past its limit (now `used + need > limit`); `update_counter` and `update_element` never
  checked the limit (now they do, as inserts).
- `resize_resource` adjusts the caller's heap only if its own heap holds the resource
  (`Heap::holds`): a persistent_term value resized through its literal is not the caller's.

## Tests (exact commands, exit codes)
- `q run --cores 8 -- cargo test -p beamlet-vm` (userland/otp): rc 0, 99 tests.
- New (vm/tests/sized.rs, limits 2^20 words): a_128_mb_atomics_array_in_persistent_term_past_the_limit_is_refused,
  a_replaced_persistent_value_still_counts, a_zlib_stream_grown_in_persistent_term_past_the_limit_refuses_the_next_put,
  a_zlib_stream_grown_in_ets_past_the_limit_refuses_the_next_insert,
  deleting_the_table_holding_a_grown_stream_makes_room, an_update_element_past_the_ets_limit_is_refused.
- Old code (HEAD's six VM sources, only the Limits field added): all six FAIL, the rest pass
  (`.tmp/BEAM17/old-code.txt`): `{ok,#Ref}`, `20`, `ok`, `{true,true}` x2, `{true,[{k,#Ref}]}`.
- `q run --quiet -- cargo test -p beamlet-redoubt --features fake --test limits`: rc 0, 7 passed.
- `make -f scripts/jobs.mk docs`: PASS, rc 0.
- `q run --cores 8 -- ./test-shell`: every stage passed, rc 0.
- Full difftest (`tools/difftest`, all suites): 518/524, rc 1; the 6 failures are exactly B34's
  atomvm ones on main (test_binary_to_term, test_code_all_available_loaded, test_code_server_nifs,
  test_display_string, test_node, test_unicode). None new.
- `make -f scripts/jobs.mk prebuilt`: rc 0.
- `make -k -f scripts/jobs.mk rv64/formatting rv64/no-cruft rv64/unsafe-budget`: all PASS, rc 0.
- `make -k -f scripts/jobs.mk set CASES="$(scripts/shell-cases origin/main)"` (beamlet's set, 30
  cases on each width, incl. beamlet-files/boot/footprint/heap-flood/budget-flood, userland-boot,
  userland-read-only): 60/60 PASS, rc 0. No reruns needed.
- Logs: /home/mcloonan/redoubt/.tmp/BEAM17/{test-shell,difftest,prebuilt,checks,set}.log.
- Not run: the whole bench (train's).

## Measurement (shell at its prompt, ./shell --fake)
`:erlang.memory(:ets)` = 3,696 bytes (10 tables); `:persistent_term.info()` = 20 keys, 36,112
bytes counted. A session's sixteenth (11,904 pages / 16) = 744 pages = 3,047,424 bytes: ETS
uses 0.12 %, persistent_term 1.2 %; margins about 820x and 84x. On the page (beamlet.md, Limits
inside one VM). Output: `.tmp/BEAM17/shell-measure.txt`.

## Documentation check
- docs/userland/beamlet.md, Limits inside one VM: Process memory bullet's residual rewritten
  (stored resources now count toward ETS/persistent_term; only a message not yet received counts
  nowhere); ETS bullet (live size, update_* checked, costs); new persistent_term bullet
  (cumulative, never freed, key words released, info() memory); Redoubt paragraph (three
  sixteenths, measurement); status list +6, tested (32).
- docs/testbench.md (beamlet-footprint paragraph): "process heap, ETS and persistent_term limits".
- docs/todo/beamlet-budget-from-startup.md: same three limits.
- redoubt/src/lib.rs `limits()` doc comment; redoubt/tests/limits.rs module doc.
- Checked, no change needed: README.md, GETTING-STARTED.md, docs/SECURITY.md, docs/plan/*
  (no claim about ETS/persistent_term limits; grep for max_ets_words, persistent_term, sixteenth).
  docs/kernel/budgets.md mentions the 11,904-page session only.

## Open risks
- ETS heir data (`{heir, Pid, Data}`) is kept per table and counted by no limit (pre-existing;
  bounded by 8,192 tables x term size). Not in scope; a follow-up if wanted.
- A resource in a message not yet received counts toward no limit (stated on the page).
- persistent_term:info() memory now reports the counted words, replaced values included,
  where BEAM reports live memory.

## Round 2 (kernel red's notes, folded into the VM commit)

Head 4b6d80a5c on main c01196f07 (contains 93f933c7a):
- 39b1bfdc6 beamlet: persistent_term has a word limit, and ETS and persistent_term count resources live
- 4b6d80a5c beamlet: persistent_term gets a sixteenth of the VM's budget

1. Heir data counted: `Table.heir` private, set only through `Table::set_heir`, which moves the
   old data's term words and resources out of the count and the new data's in. `ets:new` and
   `ets:setopts` check room for heir data (only when there is some: `{heir, none}` and a table
   without heir are never refused). Drop leaves objects and heir data; clear keeps the heir's.
2. `Tables::create` moves whatever the new table holds (objects and heir data) from its own
   Holdings into the shared one; no assert. A table refused there leaves the shared count as
   it was when dropped.
Tests: heir_data_past_the_ets_limit_is_refused, heir_data_counts_toward_the_ets_limit; both fail
on the old code (`.tmp/BEAM17/old-code-heir.txt`: `{#Ref,true}`, `{true,true}`). Page: ETS
bullet + 2 status entries (tested (34)).

Gates on 4b6d80a5c (logs `.tmp/BEAM17/r2/`): beamlet-vm rc 0 (sized 18/18); ./test-shell every
stage passed; prebuilt rc 0; docs, formatting, no-cruft, unsafe-budget PASS; beamlet set 60/60
on both widths, rc 0; full difftest 527/527, rc 0.
