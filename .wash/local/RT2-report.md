# RT2 report: complete

Branch wp-rt2, base main 665ec84c3, tip a48e430a7. Three commits:
- daf455e76 rt: serve, the one receive loop of a server that takes only calls
- 137cec28d rt: close_delivery, for the receive loops that keep their own shape
- a48e430a7 docs: serving.md names serve, the one receive loop

## Delivered
- `redoubt_rt::server::serve(&Endpoint, FnMut(Request) -> R) -> u32`; `close_delivery(&Delivery)`;
  `exit::RECEIVE_FAILED = 3`.
- serve callers: keyd, bootfsd, blkd, fsd, echo-server (loop only in fsd/blkd).
- close_delivery + exit::RECEIVE_FAILED: consoled, netd (bins), ipd (bin; close_delivery in
  `src/server.rs` on_send, where ipd closed a send's handles).
- Red round 1 notes, folded into the serve commit:
  (1) serve's doc states the drop rule (f answers every call and returns only how the reply went;
  a handler whose error still needs a reply or undo does not use serve; a dropped Request is
  answered Malformed by its Drop). Each of the five call sites says why its result is dropped.
  R left generic: the rule is stated, not typed.
  (2) Keeper `serve_survives_abandoned_and_exit_notices`: a call kept by f and timed out by its
  caller gives Abandoned; a child created on the endpoint and exited by the fake gives Exit; the
  next call is answered; 5 receives counted. Interrupt: the fake and the kernel return it only to
  a receive on an IRQ handle, never an endpoint's, so it cannot be delivered (said in the test's
  doc and serve's). Mutations returning on Abandoned, or on Exit, fail it.
  (3) `exit`'s doc lists 0, 3, 101, 102 and says servers number their own 2, then from 4.
- Page: `### serve` first under Interface, details status naming the four keepers, the bullet
  verbatim (names in backticks). Authority's audit bullet on Send arms now names serve and
  close_delivery.

## Gates (in-dev cargo testbench, all exit 0)
At the tip: rt-host-tests, blkd/fsd/r4/client/netd/ipd/net-host-tests; rt, keyd, bootfsd, blkd,
fsd, consoled, netd, ipd -build (rv64+rv32); size-budget, unsafe-budget, no-cruft; doccheck 0;
fmt --check 0 on all eight crates; init-servers, net-tcp.
On the code before the comment-only notes: bench-console-after-expect, bench-net-peer,
bench-net-self-unrefused, init-boot, init-console-forgery, init-refuses-consoled-handed,
init-restart, init-rollback, net-attacks, net-pinned, net-tcp, netd-restart, panic-in-print,
uart-irq, init-servers, sshd-loopback-interrupt, -r67, -window-change-zero, -window-change,
init-refuses-second-keyd, -held-login-key, -held-bundle-key, bench-virtio-legacy-off.
Commit 1 alone: redoubt-rt host tests pass, all seven servers' tests build.
No whole bench (awaiting your word).

## Sizes (lines; each server falls)
keyd 556->539 (-17), bootfsd 242->227 (-15), blkd 1579->1562 (-17), fsd 1172->1156 (-16),
consoled 343->339 (-4), netd 1592->1590 (-2), ipd 2890->2887 (-3); libs/rt 2920->2927 (+7),
with Size budget lines in commits 1 and 2. Unsafe unchanged (rt 10, blkd 4, netd 8, rest 0).

## Not done, for the record
init.rs and libs/client/src/launch.rs carry the same handle-closing loop in their Send arms;
outside this node's list, left alone.
