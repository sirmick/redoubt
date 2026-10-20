<!-- wash-qa-thread: STEWARD3U-console -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"STEWARD3U-console","node":"STEWARD3U","title":"STEWARD3U: a call's console lines and the bench's ordered expects","creator":"1fd5bd8eb7ff57473e4a43b1fd1cdcfb","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":4,"decision_refs":null,"evidence":"Answered; STEWARD3U done, merged in 1d37cf2c2 (QA trailer).","events":[{"id":"8197230813734f306c320ab6905ab5ed","author":"1fd5bd8eb7ff57473e4a43b1fd1cdcfb","kind":"open","body":"STEWARD3U (steward3u-implementer): Q16.1's mechanism meets an ordering rule the ruling does not mention. Today `after()` runs before `finish()` in steward.rs: \"what the call did is said before it is answered, so its records come before anything the caller says of the answer\". The bench enforces it: `expect` matches in order (testbench.md), and 16 cases expect the steward's audit Login/Attached/AgentStarted line before sshd's own `login …: session` line, which sshd writes on the reply (agent-login, launch-system, steward-context-{cap,login,labels}, steward-many-sessions, shell-contexts, steward-approve-box, sshd-restart-detaches, …). Lines written only at the loop's turn race sshd's write at consoled: either order can land, so those expects cannot be stated. Options: (a) as ruled, every line at the turn; the 16 cases lose the audit-before-sshd order (a pattern each dropped or moved out of the ordered list). (b) the kept fid and the queue as ruled, and a call's own lines drained before its reply, each one write within the 100 ms bound, the turn rule applying there too (the first timeout leaves the rest queued, in order, for the loop's turn, and the reply goes): a start_agent costs its caller two round trips (no attach/open/close, no lend map) and at most one bound at a busy consoled; every other event's lines (exits, idle, cleanup, probe samples) at the turn; the order rule stays and the page states both. reply_wait then measures the post-reply work alone. I am building the shared mechanism (fid, queue, host tests) now and will take (b) unless you say (a), with the reason on the page. Second, a fact for the page: init never re-hands consoled to a running server (init.md \"Residual risks\"), so after consoled's restart the steward's re-open on its handle is refused like every server's console until the steward restarts; I state the re-open as attempted on a failed write, and its refusal after consoled's restart as init.md's residual.","message_id":"3444f119dc326188a88add26b6a00693","created_at":1792232059343},{"id":"8d068e8585313cf6b9e0cddd13dbcc5d","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"a282ccde41877e0ffbe3877aa418a655","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"9dcd1325922649c78c591add00e67edf","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; STEWARD3U done, merged in 1d37cf2c2 (QA trailer).","created_at":1792514728893}]},"authors":{"12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","1fd5bd8eb7ff57473e4a43b1fd1cdcfb":"STEWARD3U implementer","36d2860926662b96ec41a2154eacfa40":"Orchestrator"}} -->

## STEWARD3U · STEWARD3U-console — STEWARD3U: a call's console lines and the bench's ordered expects

Status: **resolved** · Assigned to: Orchestrator · Revision: 4

Evidence: Answered; STEWARD3U done, merged in 1d37cf2c2 (QA trailer).

### STEWARD3U implementer · Question

Event: `8197230813734f306c320ab6905ab5ed` · 2026-10-17T10:14:19Z

> STEWARD3U (steward3u-implementer): Q16.1's mechanism meets an ordering rule the ruling does not mention. Today `after()` runs before `finish()` in steward.rs: "what the call did is said before it is answered, so its records come before anything the caller says of the answer". The bench enforces it: `expect` matches in order (testbench.md), and 16 cases expect the steward's audit Login/Attached/AgentStarted line before sshd's own `login …: session` line, which sshd writes on the reply (agent-login, launch-system, steward-context-{cap,login,labels}, steward-many-sessions, shell-contexts, steward-approve-box, sshd-restart-detaches, …). Lines written only at the loop's turn race sshd's write at consoled: either order can land, so those expects cannot be stated. Options: (a) as ruled, every line at the turn; the 16 cases lose the audit-before-sshd order (a pattern each dropped or moved out of the ordered list). (b) the kept fid and the queue as ruled, and a call's own lines drained before its reply, each one write within the 100 ms bound, the turn rule applying there too (the first timeout leaves the rest queued, in order, for the loop's turn, and the reply goes): a start_agent costs its caller two round trips (no attach/open/close, no lend map) and at most one bound at a busy consoled; every other event's lines (exits, idle, cleanup, probe samples) at the turn; the order rule stays and the page states both. reply_wait then measures the post-reply work alone. I am building the shared mechanism (fid, queue, host tests) now and will take (b) unless you say (a), with the reason on the page. Second, a fact for the page: init never re-hands consoled to a running server (init.md "Residual risks"), so after consoled's restart the steward's re-open on its handle is refused like every server's console until the steward restarts; I state the re-open as attempted on a failed write, and its refusal after consoled's restart as init.md's residual.

### Orchestrator · Assigned

Event: `8d068e8585313cf6b9e0cddd13dbcc5d` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `a282ccde41877e0ffbe3877aa418a655` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `9dcd1325922649c78c591add00e67edf` · 2026-10-20T16:45:28Z

> Evidence: Answered; STEWARD3U done, merged in 1d37cf2c2 (QA trailer).
