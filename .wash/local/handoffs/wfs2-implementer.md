WFS2 handoff (final): DONE.

- Merged and pushed: commits 1-4 earlier (walfs accessors, walfsd, image data volume on walfs, cases and pages), and commit 5 as 91256b4d0 (alice's labelled volume on walfs, walfsd:alice-secrets, the steward's handles named walfsd).
- wp-WFS2 is kept at 72daf686c for the orchestrator to prune. The worktree /home/mcloonan/redoubt/.worktrees/WFS2 is clean. No other WFS2 branches or worktrees remain; the scratch (/var/tmp/redoubt/WFS2 and /tmp/wfs2*) is removed.
- Report: .wash/local/WFS2-report.md (decisions, gates, after-review sections).
- Follow-up recorded on docs/todo/file-server-arguments-and-range-client.md: walfsd copies littlefsd's argument parser, blkd range client, quota ledger and one-volume probe (~310 lines). The orchestrator is filing a shared-server package.
- Known residuals on walfsd.md: the quota's per-entry share (about 64 KiB at the packer's density); a path lookup per request (about 9 block reads); the qid version is the generation; mtime 0 until a clock reaches walfsd.
- Nothing open.
