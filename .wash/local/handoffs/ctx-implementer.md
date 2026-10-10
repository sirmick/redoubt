# Handoff: ctx-implementer (CTX2 in progress; CTX1 head f7ae638b1 awaiting merge)

## Branch state (nothing pushed)
- CTX1: wp-CTX1 f7ae638b1 (2 commits on main 5151adaf7), all gates green, sent for merge. Worktree .worktrees/CTX1.
- CTX2: worktree /home/mcloonan/redoubt/.worktrees/CTX2, branch wp-CTX2 stacked on wp-CTX1 f7ae638b1. WIP commit 06dfab6da (phase 1 + wire). Rebase onto main when CTX1 merges (orchestrator says when).
- Design + decisions + implementation plan: .wash/local/CTX2-design.md (read "Decisions", "red's attack notes", "Implementation plan").

## Done in 06dfab6da
- Core: Session.attachment/from; Index.attachments; state Detached; events Attach (raised by take_over after login_key/owns_labels/not_locked/context_free), Detach (from SshdGone), ChannelClosed routed by attachment id; steps LaunchRelay/Attach/Detach (slots RELAY 200, CONSOLE 201); effects launch_relay, attach_relay, detach_relay (=detach_with(cx,true)), take_over, refuse_in_use, audit_attached; Record::Attached; notes built in core; reply_login answers s.attachment (first = session id). No current_attachment guard: detach_relay forgetting the id is R80's keeper.
- Model: P18 (both_attached, notes printable, Running<=>attached, stale close changes nothing, Attached key check), ops ChannelClosed{session,back}/SshdGone, Login from (FROMS), session size processes 2; mutations PolicyTakeoverBeforeAuth, PolicyBothAttached, PolicyStaleCloseDetaches (R80), ALL 158; new scenario declassify_live (PolicyDeclassifyLive now past seed 500). Model work() uses the PROCESS token (not the relay).
- Reference (Elixir) mirrors all; context.trace hand trace; unreachable-session-running.trace deleted (no Running unreachable row). run-traces rc 0, all rows taken.
- Wire: steward login gains `from: string`; new table libs/wire/tables/consrelay.md (attach 24, detach 25, hello 26 send, error failed); owning page docs/servers/consrelay.md NOT yet written.
- sshd: reads /tcp/N/remote on fid ctl+3, passes `from`. LIMITS buckets still 2 (decision H: 3, say why on sshd.md).
- Steward server: Kernel trait gained launch_relay/attach/detach; drive.rs runs the steps (Made::Relay(pid, control)); Recorder in tests stubs them. The machine (bin/steward.rs, target none) does NOT implement them yet -> rv64 build broken until phase 4.

## Left
- Phase 3: consrelay crate (lib + host tests + bin): creates own endpoint, mints vm + control badges, sends hello on handed hello badge; serves 9P /dev/cons to VM (like sshd console.rs Cons), consol size/resize, control attach/detach; 3 threads (server, reader, writer); 64 KiB detached buffer preallocated, drop count line, cut to first \n, writes never wait detached.
- Phase 4: steward bin: hello endpoint at start; launch_relay (Launch::streamed of "consrelay" from /boot, handle hello badge, wait <=2 s for hello, keep control, stash vm badge for the next Connect slot 4); attach (pending sshd console -> keep, new_connection for relay, call attach); detach; Event::Abandoned of watch -> EventKind::SshdGone. Server host tests (a_live_context... now takeover).
- Phase 5: consrelay on /boot (image recipe), measure pages (B32 rule), sizes.session processes 3.
- Phase 6: cases (reattach, takeover, labels, sshd restart detaches, buffer bound) + steward-context-login InUse line -> takeover; pages (steward R80, sessions, sshd, consrelay.md, consoled consol, model.md counts, testbench); re-measure reach/catch tables.
- Gates per orchestrator: sshd/steward/init host, model host (P17 kept), elixir-oracles, steward-*/sshd-*/userland-boot + beamlet set both widths, footprint, size/unsafe, docs, formatting.

## Traps
- Use rustfmt +nightly per file (edition from crate Cargo.toml), never stable cargo fmt.
- Steward reach/catch tables need re-measure (generator changed): LAST in model/tests/steward_reach.rs, counts in properties.rs, model.md.
- scripts/shell-cases: run the worktree's copy (./scripts/shell-cases).
