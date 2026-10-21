# K36 survey: holders that rely on a swept handle index vanishing

k36-implementer, 2026-10-21, on main 7575a4267 (worktree .worktrees/K36). Read only; no code yet.

## The kernel's sweeps today

Every sweep calls `remove_handle` (kernel/src/handle.rs:353), which zeroes the slot; the next
`install_handle` takes the lowest free index, so the index is reused at once.

| Sweep | Where | What it closes, in which tables |
| --- | --- | --- |
| S1 budget destruction | `destroy_marked` → `close_dependents` (budget.rs:1183) | object chain: handles naming a dying budget, or an endpoint/device charged to one; stamp chain: handles stamped with one. Held outside the subtree. |
| S2 process object freed | `free_object` → `close_handles_to` (process.rs:805) | every handle to the object, anywhere: at notice receipt or drop, or the creator's destruction |
| S3 device destroyed | `destroy_device` / `destroy_quarantined_devices` → `sweep_handles[_now]` (message.rs:1067, 1980) | every handle to the device (owner destroyed, or a DMA reset never confirmed) |
| (not a sweep) process ends | `close_all_handles` (budget.rs:689) | the ender's own table, whole: no holder is left to act by an index |

Queued message copies are nulled at delivery (`is_live`); unchanged by K36.

## Holders

Three kinds of reliance: **(A)** acts by an index that may have been swept (the hazard K36
removes); **(F)** forgets a swept index without closing it, because the kernel freed it (under
K36 each becomes a dead entry held until the process ends: a leak in a long-lived holder);
**(B)** tests "gone" by `BadHandle` (under K36 it reads `Dead`).

### init (servers/init/src/bin/init.rs) — long-lived, must change

- **own handles at each server** (`self.own`, :447-454): minted stamped with the instance's
  budget, swept (S1) at each restart; `own.retain` drops the old index unclosed. **F**: one dead
  slot per server restart, unbounded over uptime. Fix: close the old one before re-minting.
  The `ENDED` guard (:89-93, `own()`/`alive()`) is an **A** guard with a check-then-call race;
  it stays (it also stops calls to a dead instance) but no longer carries safety.
- **console connection** (`connection()`, :306-317): drops `self.console` when consoled ended
  because "its handle may have been swept and its slot reused". **A+F**: close it there.
  `Out::Console(file)` likewise.
- **the instance budget** (`BUDGETS[i]`, destroyed by the watching thread): the handle used for
  the destroy is itself in the dying budget's object chain. **F** per restart (see Q2).
- **closed-copy check** (:589-592): close then close-again = `BadHandle`. Copies are closed right
  after start; if a sweep made one dead first, close of a dead entry is Ok and the second is
  `BadHandle`, so the check holds **provided handle_close on a dead entry returns Ok** (Q3).
- **device quarantine** (:728-736): `device_info(h).is_err()` → reboot. Becomes `Dead`; still
  is_err; today it could read a reused index (a later device or another kind) and miss the
  quarantine. **B**, no change needed; init reboots, so the dead slot is moot.

### steward (servers/steward) — long-lived, must change

- **Steward::destroy** (drive.rs:674-704) and the **install lock** (`without_installs`,
  steward.rs:55-67, 791, 1357): the STEWARD10/11 + KILL1 witness. **A+B**: the guard maps
  `Err(BadHandle) if now >= deadline` to done; under K36 the call returns `Dead`, which today's
  code returns as a failed step. Must change to `Dead`. With no reuse possible the clock witness
  and the install lock are unnecessary (a dead index answers `Dead` whenever the call lands);
  retiring them is a simplification (Q5).
- **DestroyBudget** (drive.rs:820-828): `made.remove(budget)` with no close, for a lease budget
  taken by its deadline, one the steward destroyed, and a scope gone with its parent. **F** per
  session/lease/scope: must close.
- **held / notices / launched** (steward.rs:1320-1350, `Held`): send rights and connections
  stamped with a scope, "kept so a scope's end leaves no stale index here". **F** at the scope's
  end: must close.
- **handles sshd sends** (console, relay_console, approval, channels, relay_vm; protocol.rs:350,
  388, 416): stamped as sshd's were; swept if their stamp/owner goes. steward.rs:1026
  `close(stale)` on `relay_vm` can today close a reused index (**A**); under K36 it closes the
  dead entry. Fine once closes are done on every replacement.
- Host tests `a_leases_index_recycled_for_*` and `an_end_lease_after_the_deadline_*` model the
  reuse; they are rewritten to the dead entry. The steward model (model/src/steward.rs:509-515)
  uses `BadHandle` for the stale case: **B**.

### Servers holding client-stamped copies

blkd, netd, keyd, bootfsd, verityd, littlefsd, walfsd, crossing: the codec refuses a request
carrying handles and the dispatch closes them (blkd server.rs:246 and peers): **none kept**.
libs/rt's 9P server (ninep.rs:535) and parked/typed dispatch take a handle only for the call
and treat a 0 slot as malformed: unchanged. **The steward is the one system server that keeps
client-sent copies long** (above). sshd and consrelay keep connections minted *for* them, not
client-stamped copies; unaffected beyond F if their stamps go (they end with the session).

### beamlet VM (userland/otp/redoubt) and libs/client's hub

- **budget_destroy** (system.rs:495-520): marks the handles that died with the budget "gone, so
  no collection closes their indices". **F**: every launch (each pipeline stage, its endpoint
  after B106, minted connections) leaves dead slots for the session's life; pipe-sixty-four makes
  dozens. Must flip: a collected term closes its index (the dead entry), always safe now.
- **Io::forget / hub waiter** (io.rs:58-75, libs/client aio.rs completion call): **A**, the
  red's P1-1. Under K36 the waiter's call gets `Dead`: the beneficiary; the "may be another
  handle's" text goes. B106's close_served (P3-1) closes with K36.
- **pool call threads** (pool.rs:268-280): call by a raw index the BEAM side holds: **A**,
  beneficiary.
- jobs.rs `deliver` closes process and exit after the notice: correct either way.
- libs/client grants.rs: the stale-slot note concerns a caller's own close, not a sweep: no change.

### libs/rt

- `Budget::destroy(self)` (handle.rs:194) consumes the handle but leaves its index: **F** for
  every caller (init, steward, VM) unless it closes after (Q2).
- `handle::close` on a dead entry: Ok (Q3). Call paths: `Dead` already maps to
  `CallError::Disconnected` for IPC; non-IPC wrappers pass the error through.

### Model (model/)

- kernel.rs `sweep` (:1063) removes from `handles`; `handle_slots` (:1014) takes the lowest index
  not in `handles`. Plan: a per-process dead-index set: `sweep` moves indices there,
  `handle_slots` skips them, `handle_close` frees them, every lookup through one is `Dead`.
- **I1/I2** (`i1_i2_i3_i4_handles`, invariants.rs:801) read only `handles`; a dead entry names
  nothing, so they are untouched. A new contract: a swept index answers `Dead` until closed and
  is never installed over. Mutation: the sweep frees the index (today's behaviour), caught by
  the contract. No new RNG draws: gen.rs picks indices from values already drawn.

### Bench programs that expect `BadHandle` after a sweep (consumer sweep)

budget-destroy-attack, budget-deadline, budget-reap, budget-table-attack, device-owner-destroy,
dma-reset-quarantine, expiry-deadline-then-timeout, deadline-in-launch, proc-attack,
proc-review, ipc-outcomes, init-programs/keeper: each re-read; `Dead` where the index was swept.

## Does every sweep need dead entries?

**Recommend all three live-table sweeps (S1, S2, S3).** The hazard is the index, not the kind:
- S1 budget handles are the steward's STEWARD10/11/KILL1 class itself; endpoint and stamp
  sweeps are the red's P1-1. Excluding budgets would leave the install lock as the only defence.
- S2: a process handle reused for another unstarted process object lets a stale
  `process_map`/`process_start` act on the wrong child; the creator and the notice receiver both
  hold such indices.
- S3: init's quarantine check reads a possibly-reused index today.
One rule ("a sweep never frees an index") is simpler than a kind list, and its cost is the
same bound as today's table: a dead entry counts against MAX_HANDLES and keeps its page charged
until closed. `close_all_handles` stays as is.

## Questions for the Architect

1. **Scope**: all sweeps (S1-S3), as recommended, or endpoint and process sweeps only?
2. **The destroying caller's own index**: `budget_destroy(h)` sweeps h itself (it is in the dying
   budget's object chain). (a, recommended) h becomes dead like any other, and rt's
   `Budget::destroy(self)` closes it after the call, so callers need no change; (b) the kernel
   frees h as part of the call (an exception to the rule). `budget_reap` keeps h (its budget
   lives).
3. **`handle_close` on a dead entry returns Ok** (frees it), keeping init's close/close-again
   check; every other call through a dead index returns `Dead`, **including a dead index passed
   as an argument** (a message's handle list, `process_start`'s list, `mint`'s budget,
   `process_create`'s exit endpoint): the call does nothing. Order: `BadHandle` (no entry),
   then `Dead`, then `WrongObject`. Agree?
4. **Consumer closes land in K36** (init's own/console/budget, the steward's records and held
   handles, the VM's collection, rt's destroy), since every F above leaks from the day the
   kernel changes. Agree?
5. **The steward's install lock and clock witness**: K36 makes them unnecessary. Retire them in
   K36 (smaller steward, the recycled-index host tests replaced), or keep K36 to the `Dead`
   mapping and closes and retire them in a follow-up?
