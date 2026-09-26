# Endpoints cannot be reclaimed on their own

## What

No call destroys an endpoint. It lives until its owner budget is destroyed (R10 (destruction));
closing every handle to it frees nothing. The owner is always the creating process's own budget,
so each endpoint costs that budget one page until the budget goes.

## Why it matters

The cost falls only on the owner and is bounded by its page limit (R6 (charging)), so this is a
design gap, not a defect. A server that made an endpoint per client would leak a page per client
until it ends. The design answer is badges: a server serves many clients on one endpoint and
mints a badge for each ([objects](../kernel/objects.md#mint)), so per-client endpoints are not
needed. It matters only if a real server needs per-client endpoints, or when long uptime in
M5 (persist, install, share) makes a slow leak visible.

## Where

- [`kernel/src/endpoint.rs`](../../kernel/src/endpoint.rs): endpoint creation and ownership.
- [`kernel/src/message.rs`](../../kernel/src/message.rs): `destroy_endpoint`, reached only
  through budget destruction.
- The page: [objects](../kernel/objects.md#residual-risks).

## Done when

Either a server needs per-client endpoints and an endpoint becomes reclaimable on its own (with
its open calls handled as a destruction), or the servers overview states the pattern (one
endpoint per service, one badge per client) and this closes.
