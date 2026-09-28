<!-- wash-qa-thread: B2-miri-rt-ub -->

## B2 · B2-miri-rt-ub — B2: Miri reports UB in libs/rt's page view under ipd's tests; ipd's slow tests take &gt;10 min under Miri

Status: **open** · Assigned to: Orchestrator · Revision: 4

**Blocks work on its node.**

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

<!-- wash-qa-checkpoint-v2: eyJ0aHJlYWQiOnsiaWQiOiJCMi1taXJpLXJ0LXViIiwibm9kZSI6IkIyIiwidGl0bGUiOiJCMjogTWlyaSByZXBvcnRzIFVCIGluIGxpYnMvcnQncyBwYWdlIHZpZXcgdW5kZXIgaXBkJ3MgdGVzdHM7IGlwZCdzIHNsb3cgdGVzdHMgdGFrZSBcdTAwM2UxMCBtaW4gdW5kZXIgTWlyaSIsImNyZWF0b3IiOiIyN2QxYTM3NmJlYzM2NDM0NjE1ZGZkZWMxNDljN2Y2OSIsImFzc2lnbmVlIjoiZDA3NDQ0ZDdlMTRmZWUzMjc5Y2UwZDBhYjIxYTRlNjciLCJzdGF0ZSI6Im9wZW4iLCJibG9ja2luZyI6dHJ1ZSwicmV2aXNpb24iOjQsImRlY2lzaW9uX3JlZnMiOm51bGwsImV2ZW50cyI6W3siaWQiOiIxZDA3YzVmMjYzYmE4YmQ3Mzc5NmJiNjQxNjUxZTExYyIsImF1dGhvciI6IjI3ZDFhMzc2YmVjMzY0MzQ2MTVkZmRlYzE0OWM3ZjY5Iiwia2luZCI6Im9wZW4iLCJib2R5IjoiRGV0YWlsOiAuLy53b3JrdHJlZXMvYjIvLndhc2gvbG9jYWwvQjItbWlyaS5tZC5cblxuVGhlIGBjYXJnbyArbmlnaHRseSBtaXJpIHRlc3QgLXAgcmVkb3VidC1pcGRgIHJlc3VsdHMsIGZpbGUgYnkgZmlsZTpcbi0gYXJncyBhbmQgZnMgcGFzcy5cbi0gY3RsX2RlYWRsaW5lIGFuZCBncmFudF91bmRvIGhpdCBVbmRlZmluZWQgQmVoYXZpb3IgdW5kZXIgYm90aCBTdGFja2VkIEJvcnJvd3MgYW5kIFRyZWUgQm9ycm93cy4gVGhlIHJlcG9ydCBpcyBhdCBsaWJzL3J0L3NyYy9pcGMucnM6Njk6IHRoZSBydW50aW1lJ3MgYFBhZ2VzYCB2aWV3IGlzIGEgYFx1MDAyNm11dCBbdThdYCBvdmVyIG1hcHBlZCBwYWdlcywgYW5kIHRoZSBmYWtlIGtlcm5lbCAobGlicy9ydC90ZXN0cy9jb21tb24vbW9kLnJzOjI1OS8zMzIvNjEyLzY0Mykgd3JpdGVzIHRob3NlIHNhbWUgcGFnZXMgd2hpbGUgdGhlIHZpZXcgaXMgbGl2ZS4gVHJlZSBCb3Jyb3dzIHB1dHMgdGhlIHdyaXRlIGF0IGxpYnMvd2lyZS9zcmMvY29kZWMucnM6MTEzLCBjYWxsZWQgZnJvbSBpcGMucnM6MTgyLlxuLSBpc24gdGFrZXMgbW9yZSB0aGFuIDI4MCBzIGFsb25lLiBwcm9ncmFtLCBzY29wZSwgc2l6aW5nLCBzdGFjaywgc3dlZXAgYW5kIHZlY3RvcnMgaGF2ZSBub3QgcnVuIHlldC5cblxuSXQgY291bGQgYmUgYSBoYXJuZXNzIGFydGVmYWN0ICh0aGUgcmVhbCBrZXJuZWwgd3JpdGVzIG9ubHkgZHVyaW5nIGEgc3lzY2FsbCwgYWZ0ZXIgYHJlbGVhc2UoKWApIG9yIGEgcmVhbCBhbGlhc2luZyBidWcgYXJvdW5kIGNhbGwgY29tcGxldGlvbi4gQm90aCB0aGUgY29kZSBhbmQgdGhlIGhhcm5lc3MgYXJlIGxpYnMvcnQsIHdoaWNoIGlzIFMyJ3MuXG5cblF1ZXN0aW9uczpcbihhKSBSb3V0ZSB0aGUgVUIgdG8gUzI/IFRoZSBtaXJpIHBhZ2UgY2Fubm90IGNsb3NlIHdoaWxlIGlwZCdzIGhvc3QgdGVzdHMgcmVwb3J0IFVCLlxuKGIpIFNldmVyYWwgaXBkIHRlc3RzIHJ1biBmYXIgcGFzdCAxMCBtaW4gdW5kZXIgTWlyaSwgYW5kIEkgbWF5IG5vdCBiYWNrZ3JvdW5kIGEgam9iLiBPcHRpb25zOiBTMiBhZGRzIGNmZyhtaXJpKSBpdGVyYXRpb24gY2FwcyB0byB0aGUgaXBkIHRlc3RzLCB3aGljaCBhcmUgaXRzIGNvZGU7IG9yIEkgcmVjb3JkIE1pcmkgcnVucyBvbmx5IGZvciB0aGUgZmlsZXMgdGhhdCBmaW5pc2ggaW4gdW5kZXIgMTAgbWluIGFuZCBzdGF0ZSB0aGUgcmVzdCBvbiB0aGUgaXBkIHBhZ2UuIEkgcmVjb21tZW5kIHRoZSBjYXBzLlxuXG5UaGUgdmVuZG9yZWQgY3JhdGVzJyBvd24gdGVzdCBzdWl0ZXMgYXJlIG5leHQsIHN0YW5kYWxvbmU7IHRoYXQgZG9lc24ndCBkZXBlbmQgb24gKGEpIG9yIChiKS4iLCJjcmVhdGVkX2F0IjoxNzkwNTg2ODA1MjIzfSx7ImlkIjoiODY5Y2VlOWQ3ZWNjYjAyZjM0ZmM5MGQxZjVkYWIyZmQiLCJhdXRob3IiOiIyN2QxYTM3NmJlYzM2NDM0NjE1ZGZkZWMxNDljN2Y2OSIsImtpbmQiOiJyZXBseSIsImJvZHkiOiJWZW5kb3JlZCBjcmF0ZXMnIG93biB0ZXN0cyB1bmRlciBNaXJpLCBydW4gaW4gdW5lZGl0ZWQgc2NyYXRjaCBjb3BpZXMgKGRldGFpbCBpbiAud2FzaC9sb2NhbC9CMi1taXJpLm1kKTogbWFuYWdlZCAoYWxsb2MsIG1hcCkgMzAgcGFzc2VkOyBzdGFibGVfZGVyZWZfdHJhaXQgNiBwYXNzZWQ7IGhhc2gzMiBoYXMgbm8gdW5pdCB0ZXN0cy4gYnl0ZW9yZGVyLCBoZWFwbGVzcyBhbmQgc21vbHRjcCBjYW5ub3QgYnVpbGQgb2ZmbGluZSwgYmVjYXVzZSB0aGVpciBkZXYtZGVwZW5kZW5jaWVzIChxdWlja2NoZWNrLCBlbWJlZGRlZC1pbywgaWRuYSB2aWEgdXJsL3JzdGVzdCkgYXJlIG5vdCBpbiB0aGUgY2FyZ28gY2FjaGUuXG5cblF1ZXN0aW9uIChjKTogbWF5IEkgZmV0Y2ggdGhvc2UgZGV2LWRlcGVuZGVuY2llcyBmcm9tIGNyYXRlcy5pbyBpbnRvIH4vLmNhcmdvLCB1c2VkIG9ubHkgaW4gdGhlIC90bXAgY29waWVzIGFuZCBuZXZlciBpbiB0aGUgdHJlZT8gSWYgbm90LCBJJ2xsIHN0YXRlIG9uIHRoZSBpcGQgcGFnZSB0aGF0IHRoZXNlIGNyYXRlcycgb3duIHN1aXRlcyBjYW4ndCBydW4gb2ZmbGluZS5cblxuQWxzbyBkb25lIHNpbmNlIHRoZSBsYXN0IHJlcG9ydDpcbi0gd3JpdGUtb25seSBzcGxpdDogZHJvcHBlZCwgYXMgeW91IHJ1bGVkLlxuLSBzY2hlZC1ydjY0LWRlY2lzaW9uLXdha2UgdG9kbyBwYWdlIGFkZGVkIGFuZCBsaW5rZWQgZnJvbSB0aGUgbTEgcGxhbidzIEtlcm5lbCBidWxsZXQgYW5kIGZyb20gc2NoZWR1bGluZzsgaXQgYWxzbyBjb3ZlcnMgUjEwJ3MgdGhpbiBtYXJnaW4uXG4tIHNpemUtYnVkZ2V0IGNsb3NlZCAoYjExNGFiY2VhKS5cbi0gdmVyZGljdC1zdHJpbmdzIGNsb3NlZCAoY2MzNTY1YjUxKTsgYWxsIGZpdmUgcmVuYW1lZCBuZXQgY2FzZXMgcGFzcy5cblxuTm90ZSBvbiB0aGUgdGFyZ2V0czogeW91ciBwNTAgcnVsaW5nIHNhaWQgMjUvMTMwLzE2MCBtcy4gQWZ0ZXIgcmVkIG5vdGUgMiAoYSBzbWFsbCBtYXJnaW4sIHBpbiB0aGUgd29yc3Qgc2VlZCkgSSBzZXQgMjAvMTE1LzE0NSBtcywgYSAxMCUgbWFyZ2luIG9uIHNlZWQgMywgYW5kIHRoZSB0YWJsZSBzaG93cyB0aGUgb2xkIHRhcmdldHMgYmVzaWRlIHRoZSBuZXcgb25lcy4gU2F5IGlmIHlvdSB3YW50IDI1LzEzMC8xNjAgYmFjay5cblxuT25seSBtaXJpIHJlbWFpbnMsIHdhaXRpbmcgb24gKGEpLCAoYikgYW5kIChjKS4iLCJjcmVhdGVkX2F0IjoxNzkwNTg2ODk0NTAxfSx7ImlkIjoiZjAyNmEzMjBkMDFhYTA5YmUwN2VmNmFlN2YyZTU2OWUiLCJhdXRob3IiOiJkMDc0NDRkN2UxNGZlZTMyNzljZTBkMGFiMjFhNGU2NyIsImtpbmQiOiJhbnN3ZXIiLCJib2R5IjoiUnVsaW5ncy4gKGEpIFllczogdGhlIFVCIGdvZXMgdG8gUzIgKGxpYnMvcnQgaXMgdGhlaXJzLCBhbmQgdGhleSBhcmUgYXVkaXRpbmcgdGhlIHJ1bnRpbWUncyBvd25pbmcgdmlld3MpLiBXcml0ZSBpdCB1cCBub3cgYXMgZG9jcy90b2RvL3J0LXBhZ2Utdmlldy1hbGlhc2luZy5tZCAoV2hhdDogdGhlIE1pcmkgcmVwb3J0cywgYm90aCBtb2RlbHMsIHRoZSBsaW5lczsgV2h5OiBhIFx1MDAyNm11dCB2aWV3IG92ZXIgcGFnZXMgdGhhdCBhbm90aGVyIHBhcnR5IHdyaXRlcyBpcyBVQiBpZiBpdCBjYW4gaGFwcGVuIG9uIHRoZSByZWFsIGtlcm5lbCwgYW5kIGEgaGFybmVzcyBhcnRlZmFjdCBzdGlsbCBoaWRlcyByZWFsIGJ1Z3M7IFdoZXJlOyBEb25lIHdoZW46IHRoZSBpcGQgaG9zdCB0ZXN0cyBwYXNzIHVuZGVyIE1pcmksIGVpdGhlciB0aGUgZmFrZSBrZXJuZWwgb25seSB3cml0aW5nIGFmdGVyIHJlbGVhc2UoKSBvciB0aGUgdmlldyBmaXhlZCksIGxpbmtlZCBmcm9tIHRoZSBTZXJ2ZXJzIGJ1bGxldCBvZiBkb2NzL3BsYW4vbTEtc2VwYXJhdGlvbi5tZC4gVGhlIG1pcmkgcGFnZSBtYXkgdGhlbiBjbG9zZSB3aXRoIGlwZCdzIE1pcmkgcnVuIG5hbWVkIGFzIGJsb2NrZWQgb24gdGhhdCBwYWdlLiAoYikgQWRkIHRoZSBjZmcobWlyaSkgaXRlcmF0aW9uIGNhcHMgeW91cnNlbGY6IHRlc3Qtb25seSBlZGl0cyBpbiBzZXJ2ZXJzL2lwZC90ZXN0cyAoUzIgZG9lcyBub3QgdG91Y2ggdGhvc2UgZmlsZXM7IHRlbGwgbWUgaWYgYSBjYXAgbmVlZHMgYSBub24tdGVzdCBjaGFuZ2UpLiBUaGVuIHJ1biBldmVyeSBpcGQgdGVzdCBmaWxlIHVuZGVyIE1pcmkgaW4gdGhlIGZvcmVncm91bmQsIGVhY2ggd2l0aCBhIHRpbWVvdXQuIChjKSBZZXM6IGZldGNoIHRob3NlIGRldi1kZXBlbmRlbmNpZXMgZnJvbSBjcmF0ZXMuaW8gaW50byB0aGUgY2FyZ28gY2FjaGUsIHVzZWQgb25seSBpbiB0aGUgL3RtcCBzY3JhdGNoIGNvcGllczsgbm90aGluZyBhZGRlZCB0byB0aGUgdHJlZSwgQ2FyZ28ubG9jayBvciB2ZW5kb3IvLiBUYXJnZXRzOiAyMC8xMTUvMTQ1IG1zIG9uIHNlZWQgMyBzdGFuZDsgdGhleSBhcmUgdGlnaHRlciB0aGFuIG1pbmUgYW5kIGZvbGxvdyByZWQncyBub3RlLiBUaGVuIHJlcG9ydCBCMiBjb21wbGV0ZSBmb3IgaXRzIHJldmlldyByb3VuZC4iLCJtZXNzYWdlX2lkIjoiNTFiODY4NmYyY2FkM2JjY2EzYzAyNDA2NjY1YjhmNTciLCJjcmVhdGVkX2F0IjoxNzkwNTg2OTQzMDM0fSx7ImlkIjoiNDFmMjA2MDFlMWU3ODQ5Y2RkZDMwYzAxNWI2YTRjNDgiLCJhdXRob3IiOiIyN2QxYTM3NmJlYzM2NDM0NjE1ZGZkZWMxNDljN2Y2OSIsImtpbmQiOiJyZXBseSIsImJvZHkiOiJEb25lLCBmb2xsb3dpbmcgeW91ciBydWxpbmdzLlxuXG4oYSkgVGhlIFVCIGlzIHdyaXR0ZW4gdXAgYXMgZG9jcy90b2RvL3J0LXBhZ2Utdmlldy1hbGlhc2luZy5tZCBhbmQgbGlua2VkIGZyb20gdGhlIE0xIHBsYW4ncyBTZXJ2ZXJzIGJ1bGxldC4gVGhlIG1pcmkgcGFnZSBpcyBjbG9zZWQ7IGlwZC5tZCBcIlVuZGVyIE1pcmlcIiByZWNvcmRzIHRoZSBydW4gYW5kIG5hbWVzIHRoZSB0aHJlZSBibG9ja2VkIHRlc3RzOiBjdGxfZGVhZGxpbmUsIGdyYW50X3VuZG8gYW5kIHByb2dyYW0uXG5cbihiKSBjZmcobWlyaSkgY2FwcyB3ZW50IGludG8gaXBkJ3MgdGVzdHMgKGlzbiwgc2NvcGUsIHN3ZWVwKTsgdGhlIG9yZGluYXJ5IGNvdW50cyBhcmUgdW5jaGFuZ2VkLiBFdmVyeSBpcGQgdGVzdCBmaWxlIHJhbiB1bmRlciBNaXJpIGluIHRoZSBmb3JlZ3JvdW5kOiA4IHBhc3MsIGFuZCB0aG9zZSAzIHN0b3Agb24gdGhlIHJ0IHZpZXcuXG5cbihjKSBUaGUgdmVuZG9yZWQgY3JhdGVzJyBvd24gc3VpdGVzIHJhbiBmcm9tIHRoZSBjYXJnbyBjYWNoZTpcbi0gc21vbHRjcDogMTU2IHBhc3M7IHRlc3RfcmFuZG9tIGlzIHRvbyBzbG93IHVuZGVyIE1pcmkuXG4tIGhlYXBsZXNzOiAyMTYgcGFzczsgdGhlIHBvb2wgdGVzdHMgdXNlIGlubGluZSBhc3NlbWJseSwgYW5kIHNtb2x0Y3AgZG9lc24ndCB1c2UgcG9vbC5cbi0gYnl0ZW9yZGVyOiA0MTIgcGFzcy5cbi0gbWFuYWdlZDogMzAgcGFzcy5cbi0gc3RhYmxlX2RlcmVmOiA2IHBhc3MuXG4tIGhhc2gzMjogbm8gdW5pdCB0ZXN0cy5cbk5vdGhpbmcgd2FzIGFkZGVkIHRvIHRoZSB0cmVlLiIsImNyZWF0ZWRfYXQiOjE3OTA1ODk5NzQxNzh9XX0sImF1dGhvcnMiOnsiMjdkMWEzNzZiZWMzNjQzNDYxNWRmZGVjMTQ5YzdmNjkiOiJJbXBsZW1lbnRlciIsImQwNzQ0NGQ3ZTE0ZmVlMzI3OWNlMGQwYWIyMWE0ZTY3IjoiT3JjaGVzdHJhdG9yIn19 -->
