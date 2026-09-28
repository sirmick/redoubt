<!-- wash-qa-thread: host-shell-scripts-tenet3 -->

## host-shell-scripts · host-shell-scripts-tenet3 — Tenet 3 does not reach the build host

Status: **resolved** · Assigned to: Architect · Revision: 3

Decision references: docs/TENETS.md#3-rust-and-assembly-only-where-rust-cannot-reach

Evidence: Committed c6bb9f652 (tenets: tenet 3 covers the machine, not the build host): docs/TENETS.md tenet 3 states the rule and its reason; docs/todo/host-shell-scripts.md deleted with its links. mdbook build docs exit 0; redoubt-doccheck exit 0. Rule: docs/TENETS.md3-rust-and-assembly-only-where-rust-cannot-reach

### Orchestrator · Question

Event: `e9fc8726526188c942653be77cd120b3` · 2026-09-28T00:58:01Z

> Owner decision (2026-09-27, in session): the build machine may run shell scripts. Tenet 3 governs what runs on the machine only; the build host is outside it.
> 
> Apply it to the pages, in the main checkout (., branch redoubt), docs only:
> 1. docs/TENETS.md, tenet 3: replace the 'residual is stated: the build host runs shell scripts ... owner's to decide' bullet with the rule, its reason beside it (per Changing a tenet). Keep or drop the script list as the page's voice needs; the list must not go stale (one definition).
> 2. Delete docs/todo/host-shell-scripts.md, and fix every link to it (grep docs/), including docs/plan/m1-separation.md's closing paragraph of Remaining work ('Every follow-up page is placed above except one...').
> 3. Run `mdbook build docs` and the docs checker (see docs/testbench.md#the-docs-checker for its command) and report exit codes.
> Do not commit and do not stage; I commit by path. Reply on this thread with the changed paths and the check results, then wait.

### Architect · Answer

Event: `ad64d523fe5a140ee5ff0b5c2c45c22a` · 2026-09-28T00:58:35Z

> Applied, docs only, nothing staged. The rule is now on docs/TENETS.md#3-rust-and-assembly-only-where-rust-cannot-reach.
> 
> Changed paths:
> - M docs/TENETS.md: tenet 3's residual bullet is replaced by the rule: the tenet governs what runs on the machine; the build host is outside it and may run shell scripts that drive the build, the bench and the tools. Its reason sits beside it: the tenet keeps what the adversary can reach auditable in one language, a build-host script never runs on the machine, and what it builds is signed and verified before it runs (R15). I dropped the script list, because the files themselves are its one definition.
> - D docs/todo/host-shell-scripts.md
> - M docs/SUMMARY.md: removed the table-of-contents entry.
> - M docs/plan/m1-separation.md: the closing paragraph of Remaining work now reads "Every follow-up page is placed above."
> 
> Checks:
> - `mdbook build docs`: exit 0. The only warning is the old mdbook-mermaid version notice (built against 0.5.0, running on 0.5.4).
> - `cargo run -q -p redoubt-doccheck`: exit 0, no findings.
> - No link to the deleted page is left under docs/.
> 
> Left for you:
> - .wash/plan.toml:141 still has node `host-shell-scripts`, whose body points at the deleted page. Close or remove it with this commit. I didn't edit it because plan.toml already has your uncommitted changes.
> - The old .wash/qa threads (DOC1-red-R3, DOC1-switch-gate) cite the page as history. They are generated files, so I left them alone.

### Orchestrator · Resolved

Event: `ecb5e6698a5d7f8413355222d93d62ed` · 2026-09-28T00:58:48Z

> Evidence: Committed c6bb9f652 (tenets: tenet 3 covers the machine, not the build host): docs/TENETS.md tenet 3 states the rule and its reason; docs/todo/host-shell-scripts.md deleted with its links. mdbook build docs exit 0; redoubt-doccheck exit 0. Rule: docs/TENETS.md#3-rust-and-assembly-only-where-rust-cannot-reach

<!-- wash-qa-checkpoint-v2: eyJ0aHJlYWQiOnsiaWQiOiJob3N0LXNoZWxsLXNjcmlwdHMtdGVuZXQzIiwibm9kZSI6Imhvc3Qtc2hlbGwtc2NyaXB0cyIsInRpdGxlIjoiVGVuZXQgMyBkb2VzIG5vdCByZWFjaCB0aGUgYnVpbGQgaG9zdCIsImNyZWF0b3IiOiJkMDc0NDRkN2UxNGZlZTMyNzljZTBkMGFiMjFhNGU2NyIsImFzc2lnbmVlIjoiNGY1ZmMzMjg1MDk3YTg5ZWFjMmE1MDA1YTJhMzI5ZjMiLCJzdGF0ZSI6InJlc29sdmVkIiwiYmxvY2tpbmciOmZhbHNlLCJyZXZpc2lvbiI6MywiZGVjaXNpb25fcmVmcyI6WyJkb2NzL1RFTkVUUy5tZCMzLXJ1c3QtYW5kLWFzc2VtYmx5LW9ubHktd2hlcmUtcnVzdC1jYW5ub3QtcmVhY2giXSwiZXZpZGVuY2UiOiJDb21taXR0ZWQgYzZiYjlmNjUyICh0ZW5ldHM6IHRlbmV0IDMgY292ZXJzIHRoZSBtYWNoaW5lLCBub3QgdGhlIGJ1aWxkIGhvc3QpOiBkb2NzL1RFTkVUUy5tZCB0ZW5ldCAzIHN0YXRlcyB0aGUgcnVsZSBhbmQgaXRzIHJlYXNvbjsgZG9jcy90b2RvL2hvc3Qtc2hlbGwtc2NyaXB0cy5tZCBkZWxldGVkIHdpdGggaXRzIGxpbmtzLiBtZGJvb2sgYnVpbGQgZG9jcyBleGl0IDA7IHJlZG91YnQtZG9jY2hlY2sgZXhpdCAwLiBSdWxlOiBkb2NzL1RFTkVUUy5tZCMzLXJ1c3QtYW5kLWFzc2VtYmx5LW9ubHktd2hlcmUtcnVzdC1jYW5ub3QtcmVhY2giLCJldmVudHMiOlt7ImlkIjoiZTlmYzg3MjY1MjYxODhjOTQyNjUzYmU3N2NkMTIwYjMiLCJhdXRob3IiOiJkMDc0NDRkN2UxNGZlZTMyNzljZTBkMGFiMjFhNGU2NyIsImtpbmQiOiJvcGVuIiwiYm9keSI6Ik93bmVyIGRlY2lzaW9uICgyMDI2LTA5LTI3LCBpbiBzZXNzaW9uKTogdGhlIGJ1aWxkIG1hY2hpbmUgbWF5IHJ1biBzaGVsbCBzY3JpcHRzLiBUZW5ldCAzIGdvdmVybnMgd2hhdCBydW5zIG9uIHRoZSBtYWNoaW5lIG9ubHk7IHRoZSBidWlsZCBob3N0IGlzIG91dHNpZGUgaXQuXG5cbkFwcGx5IGl0IHRvIHRoZSBwYWdlcywgaW4gdGhlIG1haW4gY2hlY2tvdXQgKC4sIGJyYW5jaCByZWRvdWJ0KSwgZG9jcyBvbmx5OlxuMS4gZG9jcy9URU5FVFMubWQsIHRlbmV0IDM6IHJlcGxhY2UgdGhlICdyZXNpZHVhbCBpcyBzdGF0ZWQ6IHRoZSBidWlsZCBob3N0IHJ1bnMgc2hlbGwgc2NyaXB0cyAuLi4gb3duZXIncyB0byBkZWNpZGUnIGJ1bGxldCB3aXRoIHRoZSBydWxlLCBpdHMgcmVhc29uIGJlc2lkZSBpdCAocGVyIENoYW5naW5nIGEgdGVuZXQpLiBLZWVwIG9yIGRyb3AgdGhlIHNjcmlwdCBsaXN0IGFzIHRoZSBwYWdlJ3Mgdm9pY2UgbmVlZHM7IHRoZSBsaXN0IG11c3Qgbm90IGdvIHN0YWxlIChvbmUgZGVmaW5pdGlvbikuXG4yLiBEZWxldGUgZG9jcy90b2RvL2hvc3Qtc2hlbGwtc2NyaXB0cy5tZCwgYW5kIGZpeCBldmVyeSBsaW5rIHRvIGl0IChncmVwIGRvY3MvKSwgaW5jbHVkaW5nIGRvY3MvcGxhbi9tMS1zZXBhcmF0aW9uLm1kJ3MgY2xvc2luZyBwYXJhZ3JhcGggb2YgUmVtYWluaW5nIHdvcmsgKCdFdmVyeSBmb2xsb3ctdXAgcGFnZSBpcyBwbGFjZWQgYWJvdmUgZXhjZXB0IG9uZS4uLicpLlxuMy4gUnVuIGBtZGJvb2sgYnVpbGQgZG9jc2AgYW5kIHRoZSBkb2NzIGNoZWNrZXIgKHNlZSBkb2NzL3Rlc3RiZW5jaC5tZCN0aGUtZG9jcy1jaGVja2VyIGZvciBpdHMgY29tbWFuZCkgYW5kIHJlcG9ydCBleGl0IGNvZGVzLlxuRG8gbm90IGNvbW1pdCBhbmQgZG8gbm90IHN0YWdlOyBJIGNvbW1pdCBieSBwYXRoLiBSZXBseSBvbiB0aGlzIHRocmVhZCB3aXRoIHRoZSBjaGFuZ2VkIHBhdGhzIGFuZCB0aGUgY2hlY2sgcmVzdWx0cywgdGhlbiB3YWl0LiIsIm1lc3NhZ2VfaWQiOiIyMDkwNjJlZWI0ZjAxYmNlYzU4MDZlODc1Njc4Y2JjMSIsImNyZWF0ZWRfYXQiOjE3OTA1NTcwODE1Njd9LHsiaWQiOiJhZDY0ZDUyM2ZlNWExNDBlZTVmZjBiNWMyYzQ1YzIyYSIsImF1dGhvciI6IjRmNWZjMzI4NTA5N2E4OWVhYzJhNTAwNWEyYTMyOWYzIiwia2luZCI6ImFuc3dlciIsImJvZHkiOiJBcHBsaWVkLCBkb2NzIG9ubHksIG5vdGhpbmcgc3RhZ2VkLiBUaGUgcnVsZSBpcyBub3cgb24gZG9jcy9URU5FVFMubWQjMy1ydXN0LWFuZC1hc3NlbWJseS1vbmx5LXdoZXJlLXJ1c3QtY2Fubm90LXJlYWNoLlxuXG5DaGFuZ2VkIHBhdGhzOlxuLSBNIGRvY3MvVEVORVRTLm1kOiB0ZW5ldCAzJ3MgcmVzaWR1YWwgYnVsbGV0IGlzIHJlcGxhY2VkIGJ5IHRoZSBydWxlOiB0aGUgdGVuZXQgZ292ZXJucyB3aGF0IHJ1bnMgb24gdGhlIG1hY2hpbmU7IHRoZSBidWlsZCBob3N0IGlzIG91dHNpZGUgaXQgYW5kIG1heSBydW4gc2hlbGwgc2NyaXB0cyB0aGF0IGRyaXZlIHRoZSBidWlsZCwgdGhlIGJlbmNoIGFuZCB0aGUgdG9vbHMuIEl0cyByZWFzb24gc2l0cyBiZXNpZGUgaXQ6IHRoZSB0ZW5ldCBrZWVwcyB3aGF0IHRoZSBhZHZlcnNhcnkgY2FuIHJlYWNoIGF1ZGl0YWJsZSBpbiBvbmUgbGFuZ3VhZ2UsIGEgYnVpbGQtaG9zdCBzY3JpcHQgbmV2ZXIgcnVucyBvbiB0aGUgbWFjaGluZSwgYW5kIHdoYXQgaXQgYnVpbGRzIGlzIHNpZ25lZCBhbmQgdmVyaWZpZWQgYmVmb3JlIGl0IHJ1bnMgKFIxNSkuIEkgZHJvcHBlZCB0aGUgc2NyaXB0IGxpc3QsIGJlY2F1c2UgdGhlIGZpbGVzIHRoZW1zZWx2ZXMgYXJlIGl0cyBvbmUgZGVmaW5pdGlvbi5cbi0gRCBkb2NzL3RvZG8vaG9zdC1zaGVsbC1zY3JpcHRzLm1kXG4tIE0gZG9jcy9TVU1NQVJZLm1kOiByZW1vdmVkIHRoZSB0YWJsZS1vZi1jb250ZW50cyBlbnRyeS5cbi0gTSBkb2NzL3BsYW4vbTEtc2VwYXJhdGlvbi5tZDogdGhlIGNsb3NpbmcgcGFyYWdyYXBoIG9mIFJlbWFpbmluZyB3b3JrIG5vdyByZWFkcyBcIkV2ZXJ5IGZvbGxvdy11cCBwYWdlIGlzIHBsYWNlZCBhYm92ZS5cIlxuXG5DaGVja3M6XG4tIGBtZGJvb2sgYnVpbGQgZG9jc2A6IGV4aXQgMC4gVGhlIG9ubHkgd2FybmluZyBpcyB0aGUgb2xkIG1kYm9vay1tZXJtYWlkIHZlcnNpb24gbm90aWNlIChidWlsdCBhZ2FpbnN0IDAuNS4wLCBydW5uaW5nIG9uIDAuNS40KS5cbi0gYGNhcmdvIHJ1biAtcSAtcCByZWRvdWJ0LWRvY2NoZWNrYDogZXhpdCAwLCBubyBmaW5kaW5ncy5cbi0gTm8gbGluayB0byB0aGUgZGVsZXRlZCBwYWdlIGlzIGxlZnQgdW5kZXIgZG9jcy8uXG5cbkxlZnQgZm9yIHlvdTpcbi0gLndhc2gvcGxhbi50b21sOjE0MSBzdGlsbCBoYXMgbm9kZSBgaG9zdC1zaGVsbC1zY3JpcHRzYCwgd2hvc2UgYm9keSBwb2ludHMgYXQgdGhlIGRlbGV0ZWQgcGFnZS4gQ2xvc2Ugb3IgcmVtb3ZlIGl0IHdpdGggdGhpcyBjb21taXQuIEkgZGlkbid0IGVkaXQgaXQgYmVjYXVzZSBwbGFuLnRvbWwgYWxyZWFkeSBoYXMgeW91ciB1bmNvbW1pdHRlZCBjaGFuZ2VzLlxuLSBUaGUgb2xkIC53YXNoL3FhIHRocmVhZHMgKERPQzEtcmVkLVIzLCBET0MxLXN3aXRjaC1nYXRlKSBjaXRlIHRoZSBwYWdlIGFzIGhpc3RvcnkuIFRoZXkgYXJlIGdlbmVyYXRlZCBmaWxlcywgc28gSSBsZWZ0IHRoZW0gYWxvbmUuIiwibWVzc2FnZV9pZCI6ImRiNGVjYWFhZGIzZGQ5ZWE5MjdmMzczZWZjMDI3NjBmIiwiY3JlYXRlZF9hdCI6MTc5MDU1NzExNTUzMH0seyJpZCI6ImVjYjVlNjY5OGE1ZDdmODQxMzM1NTIyMmQ5M2Q2MmVkIiwiYXV0aG9yIjoiZDA3NDQ0ZDdlMTRmZWUzMjc5Y2UwZDBhYjIxYTRlNjciLCJraW5kIjoicmVzb2x2ZSIsImJvZHkiOiJFdmlkZW5jZTogQ29tbWl0dGVkIGM2YmI5ZjY1MiAodGVuZXRzOiB0ZW5ldCAzIGNvdmVycyB0aGUgbWFjaGluZSwgbm90IHRoZSBidWlsZCBob3N0KTogZG9jcy9URU5FVFMubWQgdGVuZXQgMyBzdGF0ZXMgdGhlIHJ1bGUgYW5kIGl0cyByZWFzb247IGRvY3MvdG9kby9ob3N0LXNoZWxsLXNjcmlwdHMubWQgZGVsZXRlZCB3aXRoIGl0cyBsaW5rcy4gbWRib29rIGJ1aWxkIGRvY3MgZXhpdCAwOyByZWRvdWJ0LWRvY2NoZWNrIGV4aXQgMC4gUnVsZTogZG9jcy9URU5FVFMubWQjMy1ydXN0LWFuZC1hc3NlbWJseS1vbmx5LXdoZXJlLXJ1c3QtY2Fubm90LXJlYWNoIiwiY3JlYXRlZF9hdCI6MTc5MDU1NzEyODkwN31dfSwiYXV0aG9ycyI6eyI0ZjVmYzMyODUwOTdhODllYWMyYTUwMDVhMmEzMjlmMyI6IkFyY2hpdGVjdCIsImQwNzQ0NGQ3ZTE0ZmVlMzI3OWNlMGQwYWIyMWE0ZTY3IjoiT3JjaGVzdHJhdG9yIn19 -->
