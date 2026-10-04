# BEAM2: a labelled beamlet and system.index in a confined boot (architect-13, 2026-10-03)

## The finding stands

- init.md "The confinement check" (lines 157-166, 179-185): a server's users are the servers
  handed one of its endpoints; a confined boot refuses two label sets sharing an endpoint or a
  server instance. A labelled beamlet handed `bootfsd` shares both with every unlabelled user.
- R34 (init.md 683-689): the one exception is the steward and `sshd`, three edge kinds only;
  confine.rs `EXEMPT` = ["steward", "sshd"]. "No shared data server or device is exempt" (192).
- check.rs `INIT_CALLS` and `Why` (refusal.rs 101): one entry per program init calls itself,
  `bootfsd` among them (tested by init_calls_one_of_each_server_it_calls).
- So in a confined boot no labelled domain reaches `/boot` at all, and the brief's claim that the
  check "passes as it stands" missed `bootfsd`. The owner's ruling today rested on that claim.

## Why it is the owner's

Every way out changes something the owner settled:

1. **Recommended: the index on the userland disk, pinned by its hash in the signed bundle.**
   The index becomes one more object on the userland disk (`/<sha256 of the index>`); its SHA-256
   reaches beamlet as an argument in its manifest entry, which the pack step writes into the
   staged manifest. Chain: bundle signature (R15) -> manifest -> index hash -> index -> each
   object. beamlet is no longer handed `bootfsd`, so confined and unconfined boots are one path,
   R34 and init's one-of-each rule are untouched. Changes: decision 5's "system.index in the
   bundle" becomes "system.index's hash in the bundle"; `/boot` drops `system.index`.
2. **One `bootfsd` per label set.** Keeps decision 5 literally; breaks init's one-entry-per-called-
   program rule (refusal 101), init pushes and refills on restart each instance.
3. **Exempt `bootfsd` from R34.** Contradicts today's ruling (R34 unchanged). Read-only does not
   help: a shared server instance is a shared queue, admission state and timing surface between
   label sets, which is what the server-instance kind exists to refuse.

## What BEAM2 does meanwhile

Build point 4 with the index's source behind one function (today: `/boot/system.index` through
`bootfsd`), so either answer is a small change. The confined host test stays as built (no beamlet
handed `bootfsd`). Do not write beamlet.md's confined sentence or build any option until ruled.
