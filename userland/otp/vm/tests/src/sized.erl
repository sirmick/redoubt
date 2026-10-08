%% Fixture for vm/tests/sized.rs: resources that declare their size, through the test's natives
%% probe:sized/1 and probe:resize/2, and through the VM's own atomics and zlib natives, called
%% directly (erts_internal:atomics_new/2, zlib:*_nif). Only natives are used: the tests load no OTP
%% modules.
%% Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/sized.erl
-module(sized).
-export([small/0, many/0, grown/0, atomics_held/0, atomics_past/0, counters_past/0,
         zlib_held/0, zlib_queued_past/0, zlib_codecs_past/0, zlib_stash_past/0,
         pt_atomics_past/0, pt_replaced/0, pt_zlib_grown/0, ets_zlib_grown/0, ets_table_deleted/0,
         ets_update_element_past/0, ets_heir_past/0, ets_heir_counts/0]).

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

%% The rest run with ETS and persistent_term each limited to 2^20 words (8 MB).

%% An atomics array of 2^24 cells, 128 MB, put in persistent_term: past its limit, the put is
%% system_limit and stores nothing.
pt_atomics_past() ->
    A = erts_internal:atomics_new(16777216, 1),
    Put = try persistent_term:put(big, A) catch error:E -> E end,
    {Put, persistent_term:get(big, none)}.

%% An atomics array of 800 kB put again and again under one key: a value replaced is not freed,
%% so each put counts, and the eleventh is past the limit. Gives the puts that were stored, of
%% at most twenty.
pt_replaced() -> puts(0).

puts(20) -> 20;
puts(N) ->
    try persistent_term:put(key, erts_internal:atomics_new(100000, 1)) of
        ok -> puts(N + 1)
    catch error:system_limit -> N
    end.

%% A zlib stream put in persistent_term while empty, then given 10 MB of input through the
%% value persistent_term returns: persistent_term counts it at its live size, so the next put is
%% past the limit.
pt_zlib_grown() ->
    ok = persistent_term:put(stream, zlib:open_nif()),
    ok = zlib:enqueue_nif(persistent_term:get(stream), [<<0:80000000>>]),
    try persistent_term:put(small, 1) catch error:E -> E end.

%% A zlib stream inserted in a table while empty, then given 10 MB of input: ETS counts it at its
%% live size, so the next insert is past the limit; with the stream's object deleted, there is
%% room again.
ets_zlib_grown() ->
    T = ets:new(t, [set, public]),
    Z = zlib:open_nif(),
    true = ets:insert(T, {stream, Z}),
    ok = zlib:enqueue_nif(Z, [<<0:80000000>>]),
    Refused = try ets:insert(T, {small, 1}) catch error:E -> E end,
    true = ets:delete(T, stream),
    {Refused, ets:insert(T, {small, 1})}.

%% As above, the stream in a table of its own: deleting that table makes room in the others.
ets_table_deleted() ->
    T = ets:new(t, [set, public]),
    U = ets:new(u, [set, public]),
    Z = zlib:open_nif(),
    true = ets:insert(U, {stream, Z}),
    ok = zlib:enqueue_nif(Z, [<<0:80000000>>]),
    Refused = try ets:insert(T, {small, 1}) catch error:E -> E end,
    true = ets:delete(U),
    {Refused, ets:insert(T, {small, 1})}.

%% A zlib stream holding 10 MB put into a stored object by update_element: past the limit, as an
%% insert would be, it is system_limit and the object is unchanged.
ets_update_element_past() ->
    T = ets:new(t, [set, public]),
    true = ets:insert(T, {k, none}),
    Z = zlib:open_nif(),
    ok = zlib:enqueue_nif(Z, [<<0:80000000>>]),
    Updated = try ets:update_element(T, k, {2, Z}) catch error:E -> E end,
    {Updated, ets:lookup(T, k)}.

%% A table's heir data is kept with the table: a 128 MB atomics array as heir data, given to
%% ets:new or to ets:setopts, is past the limit and system_limit.
ets_heir_past() ->
    A = erts_internal:atomics_new(16777216, 1),
    New = try ets:new(t, [{heir, self(), A}]) catch error:E1 -> E1 end,
    T = ets:new(t, [set, public]),
    Set = try ets:setopts(T, {heir, self(), A}) catch error:E2 -> E2 end,
    {New, Set}.

%% Heir data counts as an object does, at its live size: a table whose heir data is a stream
%% holding 5 MB of queued input, given 4 MB more, leaves no room for an insert; replacing the heir
%% with none makes room.
ets_heir_counts() ->
    Z = zlib:open_nif(),
    ok = zlib:enqueue_nif(Z, [<<0:40000000>>]),
    T = ets:new(t, [set, public, {heir, self(), Z}]),
    ok = zlib:enqueue_nif(Z, [<<0:32000000>>]),
    Refused = try ets:insert(T, {small, 1}) catch error:E -> E end,
    true = ets:setopts(T, {heir, none}),
    {Refused, ets:insert(T, {small, 1})}.
