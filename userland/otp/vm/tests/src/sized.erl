%% Fixture for vm/tests/sized.rs: resources that declare their size, through the test's natives
%% probe:sized/1 and probe:resize/2, and through the VM's own atomics and zlib natives, called
%% directly (erts_internal:atomics_new/2, zlib:*_nif). Only natives are used: the tests load no OTP
%% modules.
%% Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/sized.erl
-module(sized).
-export([small/0, many/0, grown/0, atomics_held/0, atomics_past/0, counters_past/0,
         zlib_held/0, zlib_queued_past/0, zlib_codecs_past/0, zlib_stash_past/0]).

-define(LIMIT, #{size => 100000, kill => true, error_logger => false}).

%% A few small resources within the process's limit: it ends normally.
small() -> limited(fun() -> hold(100, 10, []) end).

%% A thousand of 100 kB each, 100 MB against a limit of 100,000 words: the limit ends it.
many() -> limited(fun() -> hold(1000, 100000, []) end).

%% One resource of no size, then declared 10 MB: the limit ends it.
grown() ->
    limited(fun() ->
                    R = probe:sized(0),
                    ok = probe:resize(R, 10000000),
                    spin(1000),
                    R
            end).

limited(F) ->
    {Pid, Ref} = spawn_opt(F, [monitor, {max_heap_size, ?LIMIT}]),
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason end.

hold(0, _Bytes, Acc) -> length(Acc);
hold(N, Bytes, Acc) -> hold(N - 1, Bytes, [probe:sized(Bytes) | Acc]).

spin(0) -> ok;
spin(N) -> erlang:yield(), spin(N - 1).

%% An atomics array of 1,000 cells, 8 kB, within the limit: held, and it ends normally.
atomics_held() ->
    limited(fun() ->
                    A = erts_internal:atomics_new(1000, 1),
                    spin(1000),
                    A
            end).

%% One array of a million cells, 8 MB against a limit of 100,000 words (800 kB): the limit ends it.
atomics_past() ->
    limited(fun() ->
                    A = erts_internal:atomics_new(1000000, 1),
                    spin(1000),
                    A
            end).

%% A hundred counters arrays of 10,000 cells, 80 kB each: together past the limit, which ends it.
counters_past() -> limited(fun() -> counters(100, []) end).

counters(0, Acc) -> spin(1000), length(Acc);
counters(N, Acc) -> counters(N - 1, [erts_internal:counters_new(10000) | Acc]).

%% A zlib stream with 1 kB queued: held, and it ends normally.
zlib_held() ->
    limited(fun() ->
                    Z = zlib:open_nif(),
                    ok = zlib:enqueue_nif(Z, [<<0:8000>>]),
                    spin(1000),
                    Z
            end).

%% A stream with 10 MB queued. The binary queued is off the heap and not counted; the queue is
%% the stream's, and counts: the limit ends it.
zlib_queued_past() ->
    limited(fun() ->
                    Z = zlib:open_nif(),
                    ok = zlib:enqueue_nif(Z, [<<0:80000000>>]),
                    spin(1000),
                    Z
            end).

%% Six deflate streams with nothing queued: each codec's state counts, about 230 kB, most of it
%% the tables its compressor keeps behind boxes of its own; the six pass the limit, which ends the
%% process. Counting only the compressor's inline part, about 66 kB, would leave them under it.
zlib_codecs_past() -> limited(fun() -> codecs(6, []) end).

codecs(0, Acc) -> spin(1000), length(Acc);
codecs(N, Acc) ->
    Z = zlib:open_nif(),
    ok = zlib:deflateInit_nif(Z, 6, 8, 15, 8, 0),
    codecs(N - 1, [Z | Acc]).

%% A stream whose stash holds a list of 200,000 integers, 3.2 MB, built and stashed by a process
%% with no limit and then sent to one with the limit: the stash is the stream's, and the limited
%% process that receives the stream is ended by it.
zlib_stash_past() ->
    {Pid, Ref} = spawn_opt(fun() -> receive {stream, Z} -> spin(1000), Z end end,
                           [monitor, {max_heap_size, ?LIMIT}]),
    Z = zlib:open_nif(),
    ok = zlib:setStash_nif(Z, seq(200000, [])),
    Pid ! {stream, Z},
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason end.

seq(0, Acc) -> Acc;
seq(N, Acc) -> seq(N - 1, [N | Acc]).
