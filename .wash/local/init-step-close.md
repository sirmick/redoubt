# The init step's close: M1 page edit (architect-10)

Apply on main after INIT4 merges, and only once netd-restart passes in the bench with its feature
built. Re-check first:
- init.md has no planned section except "A worked configuration". The partly-tested clauses left
  are only the steward's, `sshd`'s, step 6 and an `fsd` for each volume.
- init.md:430's "a killed `netd`'s restart has no case yet" is gone, and bench:netd-restart is in
  that list.

All edits are to `docs/plan/m1-separation.md`.

## 1. Remaining work: the "`init` and the boot manifest" bullet goes

Delete the whole bullet, from "- **`init` and the boot manifest.**" through
"([starting a case's programs](../testbench.md#starting-a-cases-programs))."

## 2. Progress: the kernel bullet

Replace:
> - **The kernel**, except SUM and MXR clearing and the three places `init` takes over from the
>   bench (what the loader loads, the budget tree, which process gets which device): handles and

with:
> - **The kernel**, except SUM and MXR clearing: handles and

Then rewrap the paragraph.

## 3. Progress: the drivers bullet

Replace:
>   `netd` and `ipd` on the real kernel through a test rig ([blkd](../servers/blkd.md),

with:
>   `netd` and `ipd` on the real kernel under `init` ([blkd](../servers/blkd.md),

## 4. Progress: a new bullet after "**Launching:** ..."

> - **`init` and the boot manifest:** the loader loads only the kernel and `init`; `init` checks
>   the manifest, builds the budget tree, hands each server its devices, starts every server
>   through the loader stub with fresh connections, and restarts a server that ends, a driver on
>   its reset device, or reboots when one cannot stay up; every server's case boots `init`
>   ([init](../servers/init.md), [starting a case's programs](../testbench.md#starting-a-cases-programs)).

## 5. Progress: the bootfsd/consoled/keyd bullet

Replace:
> - **`bootfsd`, `consoled` and `keyd`**, served and attacked in host tests

with:
> - **`bootfsd`, `consoled` and `keyd`**, attacked in host tests and booted under `init`

## 6. "Not built"

Replace:
> Not built: `init`'s manifest handling, the `fsd` server, the steward, `sshd` on the box, sessions
> and the agent.

with:
> Not built: the `fsd` server, the steward, `sshd` on the box, sessions and the agent.

(The fsd step corrects "the `fsd` server" when it closes.)

## 7. Attack suite: the R33 row's last cell

Replace `not yet` with:
> `init-refuses-budget-handle`; that no server can destroy a session is the steward's, not yet

## 8. Attack suite: the R31/R32 row's last cell

Replace "; from a user parent not yet" with:
> ; from a user parent, the steward's, not yet

## Then

Run doccheck, and stage `docs/plan/m1-separation.md` by path. Commit message:
"docs: M1's init step is built: init boots every server from the manifest". The step closes when
this commit is on main.
