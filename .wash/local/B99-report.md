# B99 report: bootfsd's heap cap and the per-request term

Branch wp-B99 in .worktrees/B99. Base 3e6f29443, head d89cca168: one commit. Tier B. bootfsd's
code is unchanged except two comments in servers/bootfsd/src/server.rs; the only other code edits
are test constants (the image entry's numbers).

## What the term is

The 9P skeleton keeps its records on the heap, while admission costs their units in the budget
(COST). A boot peak leaves them out. Record sizes, measured on the host with a throwaway test in a
scratch worktree (x86_64, the same layout as rv64; rv32's are no larger):

- Pending (a request) 216
- Session (a completion call) 784
- minted Entry (a connection) 384
- Fids (a fid holder) 192
- (u32, Fid) 56; a step 32, so the steps block is the 128 class
- admission's bucket entry 192, share entry 200
- (Stored, usize) 40; Buffer 24
- The heap has no realloc: a move holds the old block and the new one.

Per bucket at its caps:

- Tables: 64 x 216 + 2 x 784 + 23 x 384 + 47 x 192 + (192 + 24 x 200) = 38,240 bytes. Twice
  that for doubling: 76,480.
- Each fid's own blocks (its list slot, at most 256 per fid, and its steps 128) x 47: 18,048.
- Arc records of transfers: 2 x 64 = 128.
- The old block of a moving table (at most the requests'): 13,824.
- Total: 108,480 bytes a bucket.

Once: the read buffer, MSIZE 64 KiB, with its move, 32 pages; 7 pages of rounding; 8 part pages
of the small classes: 47 pages.

W(N) = ceil(108,480 N / 4096) + 47. W(4) 153, W(5) 180, W(6) 206, W(10) 312.

## Caps applied (twice the boot peak + W(buckets))

| Manifests | Buckets | Cap |
| --- | ---: | --- |
| image, boot-profile/manifest-unverified, steward/{confined, latency, many-sessions, three-contexts, vault-launch} | 6 | 2,304 + 206 = 2,510 |
| steward/idle, pipe | 5 | 2,304 + 180 = 2,484 |
| steward/six-principals | 10 | 2,304 + 312 = 2,616 |
| beamlet/{launch, natives, natives-attack, pipe, serve} | 4 | 2,176 + 153 = 2,329 |
| beamlet-footprint, beamlet-compile-footprint | 4 | 2 x 2 + 153 = 157 |

- The five beamlet/*.json manifests keep their 2,176 base unmeasured: none of their cases scans
  memory.
- The footprint manifests were 56, but bootfsd's measured peak there is 2 pages, so their base is
  now 2 x 2.
- Every budget still holds its buckets (host-tests' every_manifest_entry_holds_its_buckets_at_their_caps).
  The image entry now leaves 1,458 pages of budget, 34 buckets (the_image_entry_holds_its_buckets).

## Measured (bootfsd heap peak of cap)

| Case | rv32 | rv64 |
| --- | ---: | ---: |
| userland-boot, userland-read-only, steward-session-ends, steward-ssh-two-principals | 1,141 of 2,510 | 935 of 2,510 |
| init-boot | 1,125 | 919 |
| beamlet-footprint, -agent-, -compile- | 2 of 157 | 2 of 157 |

- bootfsd's stack peak is 7,416 bytes on rv64 and 6,056 on rv32.
- Under load (B95's 8-case set, both widths at once, 3 repeats, 48/48 PASS): the peaks were
  identical to alone. Logs are in .tmp/B99/load/.
- B95's 1,154 came from B95's own tree. On this tree the load term was not reproduced, so the cap
  rests on the derived bound, not on a measured load delta.
- The 16-page step from init-boot to userland-boot is the 64 KiB read buffer.

## Gates (exit codes)

- `make -f scripts/jobs.mk rv64/{init,r4,client,memory}-host-tests`: 0, all PASS.
- `rv64/host-tests rv64/size-budget`: 0, PASS. These are host cases, run on one width.
- `prebuilt`: 0, twice; the second after the footprint caps changed.
- `set CASES="userland-boot init-boot"`, then in pairs userland-read-only, steward-session-ends,
  steward-ssh-two-principals, beamlet-footprint, beamlet-agent-footprint and
  beamlet-compile-footprint (both widths): all 0, all PASS.
- The 8-case load set x3: 48/48 PASS.
- `set CASES="docs formatting"`: 0, PASS. It ran on the final head d89cca168.
- The boot runs above were on the tree before the final docs-only amend (testbench.md and
  bootfsd.md prose). Manifests and code are unchanged since the last prebuilt.
- Not run: the cases of the steward, pipe and beamlet/* manifests, whose caps changed. Only their
  cap grows inside an unchanged entry, and the bench's per-entry budget check passes.

## Summaries checked

- docs/servers/bootfsd.md: updated. The admission paragraph's numbers changed, and a new "Heap"
  bullet carries the arithmetic and the residual.
- docs/testbench.md memory table: the bootfsd row was stale (1,089 and 7,320). It now has 1,141,
  7,416 and 2,510, and a prose sentence.
- docs/servers/init.md "Heaps": the general rule, unchanged; no number.
- docs/servers/serving.md, docs/SECURITY.md R26: no bootfsd heap claim, so unchanged.
- README.md, GETTING-STARTED.md, docs/servers/README.md, docs/userland/beamlet.md: grep finds no
  bootfsd cap or heap claim.

## Finding (follow-up, Tier A, libs/rt)

- **The problem:** ninep_mux.rs `stored()` lists a send's T-messages before admission takes
  their pages. A transfer of up to MAX_LEND_PAGES of 7-byte messages lists 9,362 x 40 bytes: a
  vector of 655,360 bytes plus its 327,680-byte move, 240 pages, from any session.
- **Today's effect:** nothing fails, because the second boot-peak copy (about 1,150 pages) holds
  it. But a case that sent it would fail the bench's peak check, and in a server with a tight cap
  it would make other clients' allocations fail.
- **Suggested fix:** stop listing past MAX_TAGS (256). The 257th message ends the session anyway.
- Stated as a residual on bootfsd.md.

## Observation

The heap rule's doubling of the published entries costs about 1,150 pages of bootfsd's heap for
bytes that never grow. That is the rule as written (init.md "Heaps"), not changed here.
