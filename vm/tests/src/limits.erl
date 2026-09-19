%% Fixture for vm/tests/limits.rs: each function trips one resource limit and reports how the
%% VM ended the offender. Only BIFs are used: the tests load no OTP modules. Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/limits.erl
-module(limits).
-export([mailbox/0, mailbox_self/0, heap/0, heap_flag/0, heap_spawn_opt/0, heap_ok/0,
         ets/0, memory/0]).

%% Another process floods a receiver that never reads.
mailbox() ->
    {Pid, Ref} = spawn_opt(fun() -> receive never -> ok end end, [monitor]),
    flood(Pid, 1000),
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason after 1000 -> alive end.

%% A process floods itself; trapping exits does not save it.
mailbox_self() ->
    {Pid, Ref} = spawn_opt(fun() -> process_flag(trap_exit, true), flood(self(), 1000) end, [monitor]),
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason after 1000 -> alive end.

flood(_, 0) -> ok;
flood(Pid, N) -> Pid ! {msg, N}, flood(Pid, N - 1).

%% Grows a list held live on the stack until the VM-wide limit kills it.
heap() ->
    {Pid, Ref} = spawn_opt(fun() -> grow([], 1 bsl 30) end, [monitor]),
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason after 1000 -> alive end.

%% A process lowers its own limit.
heap_flag() ->
    {Pid, Ref} = spawn_opt(fun() ->
        process_flag(max_heap_size, #{size => 1000, kill => true}),
        grow([], 1 bsl 30) end, [monitor]),
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason after 1000 -> alive end.

heap_spawn_opt() ->
    {Pid, Ref} = spawn_opt(fun() -> grow([], 1 bsl 30) end, [monitor, {max_heap_size, 1000}]),
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason after 1000 -> alive end.

%% Below the limit nothing happens.
heap_ok() ->
    {Pid, Ref} = spawn_opt(fun() -> exit(length(grow([], 100))) end,
                           [monitor, {max_heap_size, 100000}]),
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason after 1000 -> alive end.

grow(L, 0) -> L;
grow(L, N) -> grow([N | L], N - 1).

%% Fills a table until inserts fail.
ets() ->
    T = ets:new(t, [set]),
    try fill(T, 0) catch error:system_limit -> {system_limit, ets:info(T, size) > 0} end.

fill(T, N) -> ets:insert(T, {N, grow([], 100)}), fill(T, N + 1).

memory() ->
    Total = erlang:memory(total),
    Procs = erlang:memory(processes),
    {memory, M} = process_info(self(), memory),
    {is_integer(Total), Procs =< Total, M > 0}.
