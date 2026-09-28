# A request answered or dropped outside `finish`

## What

`Request::reply` is public, and a `Request` closes nothing when it is dropped. The serving
library answers every call through `finish` (or `refuse`, `refuse_malformed`, or
`Parked::abandoned`), which closes the handles the request carried that do not travel; a server
may still call `Request::reply` itself, or drop a request unanswered, and the handles it carried
stay open in the server with no owner. An audit of the servers and the runtime found no such
place left ([serving](../servers/serving.md#authority)); nothing stops the next one.

## Why it matters

Each such handle is a slot in the server's table and memory spent outside the admission the
serving library keeps, so a client repeating the request grows the server's handle table: the
rule that a client cannot ([serving](../servers/serving.md#authority)) then holds only by review.

## Where

- [`libs/rt/src/ipc.rs`](../../libs/rt/src/ipc.rs): `Request`, `Request::reply`.
- [`libs/rt/src/server/typed.rs`](../../libs/rt/src/server/typed.rs): `finish`.
- The page: [serving](../servers/serving.md#residual-risks).

## Done when

- A server cannot answer a request without its carried handles being closed: `Request::reply`
  is the library's alone (`pub(crate)`), with `finish` and the refusals as the only public ways
  to answer, or `Request::reply` closes what it does not send.
- A dropped request is answered or its handles closed (a `Drop` that refuses it), or dropping
  one is made impossible to miss.
- The runtime's tests and the test programs that reply raw move to the public ways.
