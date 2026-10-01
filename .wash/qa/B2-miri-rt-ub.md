<!-- wash-qa-thread: B2-miri-rt-ub -->

## B2 · B2-miri-rt-ub — B2: Miri reports UB in libs/rt's page view under ipd's tests; ipd's slow tests take &gt;10 min under Miri

Status: **resolved** · Assigned to: Orchestrator · Revision: 6

Evidence: S2 fixed the runtime page view (merged ec675f0f6, 'a page buffer holds no reference across a system call'); docs/todo/rt-page-view-aliasing.md never reached redoubt; docs/servers/ipd.mdunder-miri on redoubt 5dd481e99 records all eleven ipd test files, ctl_deadline, grant_undo and program included, passing under Miri; B2 merged 62e7740cc.

### Implementer · Question

Event: `1d07c5f263ba8bd73796bb641651e11c` · 2026-09-28T09:13:25Z

> Detail: ./.worktrees/b2/.wash/local/B2-miri.md.
> 
> The `cargo +nightly miri test -p redoubt-ipd` results, file by file:
> - args and fs pass.
> - ctl_deadline and grant_undo hit Undefined Behavior under both Stacked Borrows and Tree Borrows. The report is at libs/rt/src/ipc.rs:69: the runtime's `Pages` view is a `&mut [u8]` over mapped pages, and the fake kernel (libs/rt/tests/common/mod.rs:259/332/612/643) writes those same pages while the view is live. Tree Borrows puts the write at libs/wire/src/codec.rs:113, called from ipc.rs:182.
> - isn takes more than 280 s alone. program, scope, sizing, stack, sweep and vectors have not run yet.
> 
> It could be a harness artefact (the real kernel writes only during a syscall, after `release()`) or a real aliasing bug around call completion. Both the code and the harness are libs/rt, which is S2's.
> 
> Questions:
> (a) Route the UB to S2? The miri page cannot close while ipd's host tests report UB.
> (b) Several ipd tests run far past 10 min under Miri, and I may not background a job. Options: S2 adds cfg(miri) iteration caps to the ipd tests, which are its code; or I record Miri runs only for the files that finish in under 10 min and state the rest on the ipd page. I recommend the caps.
> 
> The vendored crates' own test suites are next, standalone; that doesn't depend on (a) or (b).

### Implementer · Reply

Event: `869cee9d7eccb02f34fc90d1f5dab2fd` · 2026-09-28T09:14:54Z

> Vendored crates' own tests under Miri, run in unedited scratch copies (detail in .wash/local/B2-miri.md): managed (alloc, map) 30 passed; stable_deref_trait 6 passed; hash32 has no unit tests. byteorder, heapless and smoltcp cannot build offline, because their dev-dependencies (quickcheck, embedded-io, idna via url/rstest) are not in the cargo cache.
> 
> Question (c): may I fetch those dev-dependencies from crates.io into ~/.cargo, used only in the /tmp copies and never in the tree? If not, I'll state on the ipd page that these crates' own suites can't run offline.
> 
> Also done since the last report:
> - write-only split: dropped, as you ruled.
> - sched-rv64-decision-wake todo page added and linked from the m1 plan's Kernel bullet and from scheduling; it also covers R10's thin margin.
> - size-budget closed (b114abcea).
> - verdict-strings closed (cc3565b51); all five renamed net cases pass.
> 
> Note on the targets: your p50 ruling said 25/130/160 ms. After red note 2 (a small margin, pin the worst seed) I set 20/115/145 ms, a 10% margin on seed 3, and the table shows the old targets beside the new ones. Say if you want 25/130/160 back.
> 
> Only miri remains, waiting on (a), (b) and (c).

### Orchestrator · Answer

Event: `f026a320d01aa09be07ef6ae7f2e569e` · 2026-09-28T09:15:43Z

> Rulings. (a) Yes: the UB goes to S2 (libs/rt is theirs, and they are auditing the runtime's owning views). Write it up now as docs/todo/rt-page-view-aliasing.md (What: the Miri reports, both models, the lines; Why: a &mut view over pages that another party writes is UB if it can happen on the real kernel, and a harness artefact still hides real bugs; Where; Done when: the ipd host tests pass under Miri, either the fake kernel only writing after release() or the view fixed), linked from the Servers bullet of docs/plan/m1-separation.md. The miri page may then close with ipd's Miri run named as blocked on that page. (b) Add the cfg(miri) iteration caps yourself: test-only edits in servers/ipd/tests (S2 does not touch those files; tell me if a cap needs a non-test change). Then run every ipd test file under Miri in the foreground, each with a timeout. (c) Yes: fetch those dev-dependencies from crates.io into the cargo cache, used only in the /tmp scratch copies; nothing added to the tree, Cargo.lock or vendor/. Targets: 20/115/145 ms on seed 3 stand; they are tighter than mine and follow red's note. Then report B2 complete for its review round.

### Implementer · Reply

Event: `41f20601e1e7849cddd30c015b6a4c48` · 2026-09-28T10:06:14Z

> Done, following your rulings.
> 
> (a) The UB is written up as docs/todo/rt-page-view-aliasing.md and linked from the M1 plan's Servers bullet. The miri page is closed; ipd.md "Under Miri" records the run and names the three blocked tests: ctl_deadline, grant_undo and program.
> 
> (b) cfg(miri) caps went into ipd's tests (isn, scope, sweep); the ordinary counts are unchanged. Every ipd test file ran under Miri in the foreground: 8 pass, and those 3 stop on the rt view.
> 
> (c) The vendored crates' own suites ran from the cargo cache:
> - smoltcp: 156 pass; test_random is too slow under Miri.
> - heapless: 216 pass; the pool tests use inline assembly, and smoltcp doesn't use pool.
> - byteorder: 412 pass.
> - managed: 30 pass.
> - stable_deref: 6 pass.
> - hash32: no unit tests.
> Nothing was added to the tree.

### Orchestrator · Assigned

Event: `3513212df9174622a1271db483e3d7eb` · 2026-09-29T04:16:30Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `2dd6b46596eaa51cc297e0ef1d1b5f86` · 2026-09-29T04:17:19Z

> Closed at workspace reopen: the UB was the runtime's, fixed in S2; every ipd test file passes under Miri (docs/servers/ipd.md#under-miri).
> 
> Evidence: S2 fixed the runtime page view (merged ec675f0f6, 'a page buffer holds no reference across a system call'); docs/todo/rt-page-view-aliasing.md never reached redoubt; docs/servers/ipd.md#under-miri on redoubt 5dd481e99 records all eleven ipd test files, ctl_deadline, grant_undo and program included, passing under Miri; B2 merged 62e7740cc.

<!-- wash-qa-checkpoint-v2: eyJ0aHJlYWQiOnsiaWQiOiJCMi1taXJpLXJ0LXViIiwibm9kZSI6IkIyIiwidGl0bGUiOiJCMjogTWlyaSByZXBvcnRzIFVCIGluIGxpYnMvcnQncyBwYWdlIHZpZXcgdW5kZXIgaXBkJ3MgdGVzdHM7IGlwZCdzIHNsb3cgdGVzdHMgdGFrZSBcdTAwM2UxMCBtaW4gdW5kZXIgTWlyaSIsImNyZWF0b3IiOiIyN2QxYTM3NmJlYzM2NDM0NjE1ZGZkZWMxNDljN2Y2OSIsImFzc2lnbmVlIjoiYjZkZWFiMjQzYTlmNDViZjBmZjM4ZjY4MzUxNWRjYmEiLCJzdGF0ZSI6InJlc29sdmVkIiwiYmxvY2tpbmciOmZhbHNlLCJyZXZpc2lvbiI6NiwiZGVjaXNpb25fcmVmcyI6bnVsbCwiZXZpZGVuY2UiOiJTMiBmaXhlZCB0aGUgcnVudGltZSBwYWdlIHZpZXcgKG1lcmdlZCBlYzY3NWYwZjYsICdhIHBhZ2UgYnVmZmVyIGhvbGRzIG5vIHJlZmVyZW5jZSBhY3Jvc3MgYSBzeXN0ZW0gY2FsbCcpOyBkb2NzL3RvZG8vcnQtcGFnZS12aWV3LWFsaWFzaW5nLm1kIG5ldmVyIHJlYWNoZWQgcmVkb3VidDsgZG9jcy9zZXJ2ZXJzL2lwZC5tZCN1bmRlci1taXJpIG9uIHJlZG91YnQgNWRkNDgxZTk5IHJlY29yZHMgYWxsIGVsZXZlbiBpcGQgdGVzdCBmaWxlcywgY3RsX2RlYWRsaW5lLCBncmFudF91bmRvIGFuZCBwcm9ncmFtIGluY2x1ZGVkLCBwYXNzaW5nIHVuZGVyIE1pcmk7IEIyIG1lcmdlZCA2MmU3NzQwY2MuIiwiZXZlbnRzIjpbeyJpZCI6IjFkMDdjNWYyNjNiYThiZDczNzk2YmI2NDE2NTFlMTFjIiwiYXV0aG9yIjoiMjdkMWEzNzZiZWMzNjQzNDYxNWRmZGVjMTQ5YzdmNjkiLCJraW5kIjoib3BlbiIsImJvZHkiOiJEZXRhaWw6IC4vLndvcmt0cmVlcy9iMi8ud2FzaC9sb2NhbC9CMi1taXJpLm1kLlxuXG5UaGUgYGNhcmdvICtuaWdodGx5IG1pcmkgdGVzdCAtcCByZWRvdWJ0LWlwZGAgcmVzdWx0cywgZmlsZSBieSBmaWxlOlxuLSBhcmdzIGFuZCBmcyBwYXNzLlxuLSBjdGxfZGVhZGxpbmUgYW5kIGdyYW50X3VuZG8gaGl0IFVuZGVmaW5lZCBCZWhhdmlvciB1bmRlciBib3RoIFN0YWNrZWQgQm9ycm93cyBhbmQgVHJlZSBCb3Jyb3dzLiBUaGUgcmVwb3J0IGlzIGF0IGxpYnMvcnQvc3JjL2lwYy5yczo2OTogdGhlIHJ1bnRpbWUncyBgUGFnZXNgIHZpZXcgaXMgYSBgXHUwMDI2bXV0IFt1OF1gIG92ZXIgbWFwcGVkIHBhZ2VzLCBhbmQgdGhlIGZha2Uga2VybmVsIChsaWJzL3J0L3Rlc3RzL2NvbW1vbi9tb2QucnM6MjU5LzMzMi82MTIvNjQzKSB3cml0ZXMgdGhvc2Ugc2FtZSBwYWdlcyB3aGlsZSB0aGUgdmlldyBpcyBsaXZlLiBUcmVlIEJvcnJvd3MgcHV0cyB0aGUgd3JpdGUgYXQgbGlicy93aXJlL3NyYy9jb2RlYy5yczoxMTMsIGNhbGxlZCBmcm9tIGlwYy5yczoxODIuXG4tIGlzbiB0YWtlcyBtb3JlIHRoYW4gMjgwIHMgYWxvbmUuIHByb2dyYW0sIHNjb3BlLCBzaXppbmcsIHN0YWNrLCBzd2VlcCBhbmQgdmVjdG9ycyBoYXZlIG5vdCBydW4geWV0LlxuXG5JdCBjb3VsZCBiZSBhIGhhcm5lc3MgYXJ0ZWZhY3QgKHRoZSByZWFsIGtlcm5lbCB3cml0ZXMgb25seSBkdXJpbmcgYSBzeXNjYWxsLCBhZnRlciBgcmVsZWFzZSgpYCkgb3IgYSByZWFsIGFsaWFzaW5nIGJ1ZyBhcm91bmQgY2FsbCBjb21wbGV0aW9uLiBCb3RoIHRoZSBjb2RlIGFuZCB0aGUgaGFybmVzcyBhcmUgbGlicy9ydCwgd2hpY2ggaXMgUzIncy5cblxuUXVlc3Rpb25zOlxuKGEpIFJvdXRlIHRoZSBVQiB0byBTMj8gVGhlIG1pcmkgcGFnZSBjYW5ub3QgY2xvc2Ugd2hpbGUgaXBkJ3MgaG9zdCB0ZXN0cyByZXBvcnQgVUIuXG4oYikgU2V2ZXJhbCBpcGQgdGVzdHMgcnVuIGZhciBwYXN0IDEwIG1pbiB1bmRlciBNaXJpLCBhbmQgSSBtYXkgbm90IGJhY2tncm91bmQgYSBqb2IuIE9wdGlvbnM6IFMyIGFkZHMgY2ZnKG1pcmkpIGl0ZXJhdGlvbiBjYXBzIHRvIHRoZSBpcGQgdGVzdHMsIHdoaWNoIGFyZSBpdHMgY29kZTsgb3IgSSByZWNvcmQgTWlyaSBydW5zIG9ubHkgZm9yIHRoZSBmaWxlcyB0aGF0IGZpbmlzaCBpbiB1bmRlciAxMCBtaW4gYW5kIHN0YXRlIHRoZSByZXN0IG9uIHRoZSBpcGQgcGFnZS4gSSByZWNvbW1lbmQgdGhlIGNhcHMuXG5cblRoZSB2ZW5kb3JlZCBjcmF0ZXMnIG93biB0ZXN0IHN1aXRlcyBhcmUgbmV4dCwgc3RhbmRhbG9uZTsgdGhhdCBkb2Vzbid0IGRlcGVuZCBvbiAoYSkgb3IgKGIpLiIsImNyZWF0ZWRfYXQiOjE3OTA1ODY4MDUyMjN9LHsiaWQiOiI4NjljZWU5ZDdlY2NiMDJmMzRmYzkwZDFmNWRhYjJmZCIsImF1dGhvciI6IjI3ZDFhMzc2YmVjMzY0MzQ2MTVkZmRlYzE0OWM3ZjY5Iiwia2luZCI6InJlcGx5IiwiYm9keSI6IlZlbmRvcmVkIGNyYXRlcycgb3duIHRlc3RzIHVuZGVyIE1pcmksIHJ1biBpbiB1bmVkaXRlZCBzY3JhdGNoIGNvcGllcyAoZGV0YWlsIGluIC53YXNoL2xvY2FsL0IyLW1pcmkubWQpOiBtYW5hZ2VkIChhbGxvYywgbWFwKSAzMCBwYXNzZWQ7IHN0YWJsZV9kZXJlZl90cmFpdCA2IHBhc3NlZDsgaGFzaDMyIGhhcyBubyB1bml0IHRlc3RzLiBieXRlb3JkZXIsIGhlYXBsZXNzIGFuZCBzbW9sdGNwIGNhbm5vdCBidWlsZCBvZmZsaW5lLCBiZWNhdXNlIHRoZWlyIGRldi1kZXBlbmRlbmNpZXMgKHF1aWNrY2hlY2ssIGVtYmVkZGVkLWlvLCBpZG5hIHZpYSB1cmwvcnN0ZXN0KSBhcmUgbm90IGluIHRoZSBjYXJnbyBjYWNoZS5cblxuUXVlc3Rpb24gKGMpOiBtYXkgSSBmZXRjaCB0aG9zZSBkZXYtZGVwZW5kZW5jaWVzIGZyb20gY3JhdGVzLmlvIGludG8gfi8uY2FyZ28sIHVzZWQgb25seSBpbiB0aGUgL3RtcCBjb3BpZXMgYW5kIG5ldmVyIGluIHRoZSB0cmVlPyBJZiBub3QsIEknbGwgc3RhdGUgb24gdGhlIGlwZCBwYWdlIHRoYXQgdGhlc2UgY3JhdGVzJyBvd24gc3VpdGVzIGNhbid0IHJ1biBvZmZsaW5lLlxuXG5BbHNvIGRvbmUgc2luY2UgdGhlIGxhc3QgcmVwb3J0OlxuLSB3cml0ZS1vbmx5IHNwbGl0OiBkcm9wcGVkLCBhcyB5b3UgcnVsZWQuXG4tIHNjaGVkLXJ2NjQtZGVjaXNpb24td2FrZSB0b2RvIHBhZ2UgYWRkZWQgYW5kIGxpbmtlZCBmcm9tIHRoZSBtMSBwbGFuJ3MgS2VybmVsIGJ1bGxldCBhbmQgZnJvbSBzY2hlZHVsaW5nOyBpdCBhbHNvIGNvdmVycyBSMTAncyB0aGluIG1hcmdpbi5cbi0gc2l6ZS1idWRnZXQgY2xvc2VkIChiMTE0YWJjZWEpLlxuLSB2ZXJkaWN0LXN0cmluZ3MgY2xvc2VkIChjYzM1NjViNTEpOyBhbGwgZml2ZSByZW5hbWVkIG5ldCBjYXNlcyBwYXNzLlxuXG5Ob3RlIG9uIHRoZSB0YXJnZXRzOiB5b3VyIHA1MCBydWxpbmcgc2FpZCAyNS8xMzAvMTYwIG1zLiBBZnRlciByZWQgbm90ZSAyIChhIHNtYWxsIG1hcmdpbiwgcGluIHRoZSB3b3JzdCBzZWVkKSBJIHNldCAyMC8xMTUvMTQ1IG1zLCBhIDEwJSBtYXJnaW4gb24gc2VlZCAzLCBhbmQgdGhlIHRhYmxlIHNob3dzIHRoZSBvbGQgdGFyZ2V0cyBiZXNpZGUgdGhlIG5ldyBvbmVzLiBTYXkgaWYgeW91IHdhbnQgMjUvMTMwLzE2MCBiYWNrLlxuXG5Pbmx5IG1pcmkgcmVtYWlucywgd2FpdGluZyBvbiAoYSksIChiKSBhbmQgKGMpLiIsImNyZWF0ZWRfYXQiOjE3OTA1ODY4OTQ1MDF9LHsiaWQiOiJmMDI2YTMyMGQwMWFhMDliZTA3ZWY2YWU3ZjJlNTY5ZSIsImF1dGhvciI6ImQwNzQ0NGQ3ZTE0ZmVlMzI3OWNlMGQwYWIyMWE0ZTY3Iiwia2luZCI6ImFuc3dlciIsImJvZHkiOiJSdWxpbmdzLiAoYSkgWWVzOiB0aGUgVUIgZ29lcyB0byBTMiAobGlicy9ydCBpcyB0aGVpcnMsIGFuZCB0aGV5IGFyZSBhdWRpdGluZyB0aGUgcnVudGltZSdzIG93bmluZyB2aWV3cykuIFdyaXRlIGl0IHVwIG5vdyBhcyBkb2NzL3RvZG8vcnQtcGFnZS12aWV3LWFsaWFzaW5nLm1kIChXaGF0OiB0aGUgTWlyaSByZXBvcnRzLCBib3RoIG1vZGVscywgdGhlIGxpbmVzOyBXaHk6IGEgXHUwMDI2bXV0IHZpZXcgb3ZlciBwYWdlcyB0aGF0IGFub3RoZXIgcGFydHkgd3JpdGVzIGlzIFVCIGlmIGl0IGNhbiBoYXBwZW4gb24gdGhlIHJlYWwga2VybmVsLCBhbmQgYSBoYXJuZXNzIGFydGVmYWN0IHN0aWxsIGhpZGVzIHJlYWwgYnVnczsgV2hlcmU7IERvbmUgd2hlbjogdGhlIGlwZCBob3N0IHRlc3RzIHBhc3MgdW5kZXIgTWlyaSwgZWl0aGVyIHRoZSBmYWtlIGtlcm5lbCBvbmx5IHdyaXRpbmcgYWZ0ZXIgcmVsZWFzZSgpIG9yIHRoZSB2aWV3IGZpeGVkKSwgbGlua2VkIGZyb20gdGhlIFNlcnZlcnMgYnVsbGV0IG9mIGRvY3MvcGxhbi9tMS1zZXBhcmF0aW9uLm1kLiBUaGUgbWlyaSBwYWdlIG1heSB0aGVuIGNsb3NlIHdpdGggaXBkJ3MgTWlyaSBydW4gbmFtZWQgYXMgYmxvY2tlZCBvbiB0aGF0IHBhZ2UuIChiKSBBZGQgdGhlIGNmZyhtaXJpKSBpdGVyYXRpb24gY2FwcyB5b3Vyc2VsZjogdGVzdC1vbmx5IGVkaXRzIGluIHNlcnZlcnMvaXBkL3Rlc3RzIChTMiBkb2VzIG5vdCB0b3VjaCB0aG9zZSBmaWxlczsgdGVsbCBtZSBpZiBhIGNhcCBuZWVkcyBhIG5vbi10ZXN0IGNoYW5nZSkuIFRoZW4gcnVuIGV2ZXJ5IGlwZCB0ZXN0IGZpbGUgdW5kZXIgTWlyaSBpbiB0aGUgZm9yZWdyb3VuZCwgZWFjaCB3aXRoIGEgdGltZW91dC4gKGMpIFllczogZmV0Y2ggdGhvc2UgZGV2LWRlcGVuZGVuY2llcyBmcm9tIGNyYXRlcy5pbyBpbnRvIHRoZSBjYXJnbyBjYWNoZSwgdXNlZCBvbmx5IGluIHRoZSAvdG1wIHNjcmF0Y2ggY29waWVzOyBub3RoaW5nIGFkZGVkIHRvIHRoZSB0cmVlLCBDYXJnby5sb2NrIG9yIHZlbmRvci8uIFRhcmdldHM6IDIwLzExNS8xNDUgbXMgb24gc2VlZCAzIHN0YW5kOyB0aGV5IGFyZSB0aWdodGVyIHRoYW4gbWluZSBhbmQgZm9sbG93IHJlZCdzIG5vdGUuIFRoZW4gcmVwb3J0IEIyIGNvbXBsZXRlIGZvciBpdHMgcmV2aWV3IHJvdW5kLiIsIm1lc3NhZ2VfaWQiOiI1MWI4Njg2ZjJjYWQzYmNjYTNjMDI0MDY2NjViOGY1NyIsImNyZWF0ZWRfYXQiOjE3OTA1ODY5NDMwMzR9LHsiaWQiOiI0MWYyMDYwMWUxZTc4NDljZGRkMzBjMDE1YjZhNGM0OCIsImF1dGhvciI6IjI3ZDFhMzc2YmVjMzY0MzQ2MTVkZmRlYzE0OWM3ZjY5Iiwia2luZCI6InJlcGx5IiwiYm9keSI6IkRvbmUsIGZvbGxvd2luZyB5b3VyIHJ1bGluZ3MuXG5cbihhKSBUaGUgVUIgaXMgd3JpdHRlbiB1cCBhcyBkb2NzL3RvZG8vcnQtcGFnZS12aWV3LWFsaWFzaW5nLm1kIGFuZCBsaW5rZWQgZnJvbSB0aGUgTTEgcGxhbidzIFNlcnZlcnMgYnVsbGV0LiBUaGUgbWlyaSBwYWdlIGlzIGNsb3NlZDsgaXBkLm1kIFwiVW5kZXIgTWlyaVwiIHJlY29yZHMgdGhlIHJ1biBhbmQgbmFtZXMgdGhlIHRocmVlIGJsb2NrZWQgdGVzdHM6IGN0bF9kZWFkbGluZSwgZ3JhbnRfdW5kbyBhbmQgcHJvZ3JhbS5cblxuKGIpIGNmZyhtaXJpKSBjYXBzIHdlbnQgaW50byBpcGQncyB0ZXN0cyAoaXNuLCBzY29wZSwgc3dlZXApOyB0aGUgb3JkaW5hcnkgY291bnRzIGFyZSB1bmNoYW5nZWQuIEV2ZXJ5IGlwZCB0ZXN0IGZpbGUgcmFuIHVuZGVyIE1pcmkgaW4gdGhlIGZvcmVncm91bmQ6IDggcGFzcywgYW5kIHRob3NlIDMgc3RvcCBvbiB0aGUgcnQgdmlldy5cblxuKGMpIFRoZSB2ZW5kb3JlZCBjcmF0ZXMnIG93biBzdWl0ZXMgcmFuIGZyb20gdGhlIGNhcmdvIGNhY2hlOlxuLSBzbW9sdGNwOiAxNTYgcGFzczsgdGVzdF9yYW5kb20gaXMgdG9vIHNsb3cgdW5kZXIgTWlyaS5cbi0gaGVhcGxlc3M6IDIxNiBwYXNzOyB0aGUgcG9vbCB0ZXN0cyB1c2UgaW5saW5lIGFzc2VtYmx5LCBhbmQgc21vbHRjcCBkb2Vzbid0IHVzZSBwb29sLlxuLSBieXRlb3JkZXI6IDQxMiBwYXNzLlxuLSBtYW5hZ2VkOiAzMCBwYXNzLlxuLSBzdGFibGVfZGVyZWY6IDYgcGFzcy5cbi0gaGFzaDMyOiBubyB1bml0IHRlc3RzLlxuTm90aGluZyB3YXMgYWRkZWQgdG8gdGhlIHRyZWUuIiwiY3JlYXRlZF9hdCI6MTc5MDU4OTk3NDE3OH0seyJpZCI6IjM1MTMyMTJkZjkxNzQ2MjJhMTI3MWRiNDgzZTNkN2ViIiwiYXV0aG9yIjoiYjZkZWFiMjQzYTlmNDViZjBmZjM4ZjY4MzUxNWRjYmEiLCJraW5kIjoiYXNzaWduIiwiYm9keSI6IlJlc3VtZWQgZnJvbSB0aGUgUUEgZGlyZWN0b3J5OyB0aGUgb3JjaGVzdHJhdG9yIGFzc2lnbnMgdGhlIGN1cnJlbnQgdGVhbS5cbk5leHQgcmVzcG9uZGVyOiBPcmNoZXN0cmF0b3IiLCJjcmVhdGVkX2F0IjoxNzkwNjU1MzkwNjY2fSx7ImlkIjoiMmRkNmI0NjU5NmVhYTUxY2MyOTdlMGVmMWQxYjVmODYiLCJhdXRob3IiOiJiNmRlYWIyNDNhOWY0NWJmMGZmMzhmNjgzNTE1ZGNiYSIsImtpbmQiOiJyZXNvbHZlIiwiYm9keSI6IkNsb3NlZCBhdCB3b3Jrc3BhY2UgcmVvcGVuOiB0aGUgVUIgd2FzIHRoZSBydW50aW1lJ3MsIGZpeGVkIGluIFMyOyBldmVyeSBpcGQgdGVzdCBmaWxlIHBhc3NlcyB1bmRlciBNaXJpIChkb2NzL3NlcnZlcnMvaXBkLm1kI3VuZGVyLW1pcmkpLlxuXG5FdmlkZW5jZTogUzIgZml4ZWQgdGhlIHJ1bnRpbWUgcGFnZSB2aWV3IChtZXJnZWQgZWM2NzVmMGY2LCAnYSBwYWdlIGJ1ZmZlciBob2xkcyBubyByZWZlcmVuY2UgYWNyb3NzIGEgc3lzdGVtIGNhbGwnKTsgZG9jcy90b2RvL3J0LXBhZ2Utdmlldy1hbGlhc2luZy5tZCBuZXZlciByZWFjaGVkIHJlZG91YnQ7IGRvY3Mvc2VydmVycy9pcGQubWQjdW5kZXItbWlyaSBvbiByZWRvdWJ0IDVkZDQ4MWU5OSByZWNvcmRzIGFsbCBlbGV2ZW4gaXBkIHRlc3QgZmlsZXMsIGN0bF9kZWFkbGluZSwgZ3JhbnRfdW5kbyBhbmQgcHJvZ3JhbSBpbmNsdWRlZCwgcGFzc2luZyB1bmRlciBNaXJpOyBCMiBtZXJnZWQgNjJlNzc0MGNjLiIsImNyZWF0ZWRfYXQiOjE3OTA2NTU0Mzk2MzF9XX0sImF1dGhvcnMiOnsiMjdkMWEzNzZiZWMzNjQzNDYxNWRmZGVjMTQ5YzdmNjkiOiJJbXBsZW1lbnRlciIsImI2ZGVhYjI0M2E5ZjQ1YmYwZmYzOGY2ODM1MTVkY2JhIjoiT3JjaGVzdHJhdG9yIiwiZDA3NDQ0ZDdlMTRmZWUzMjc5Y2UwZDBhYjIxYTRlNjciOiJPcmNoZXN0cmF0b3IifX0= -->
