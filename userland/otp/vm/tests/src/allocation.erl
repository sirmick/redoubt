%% Fixture for vm/tests/allocation.rs: each function has one process ask the VM for an
%% allocation, larger than the test's allocator gives or than the process's own limit, and
%% reports how that process ended and that the VM went on serving. Only BIFs are used: the
%% tests load no OTP modules. Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/allocation.erl
-module(allocation).
-export([heap/0, binary/0, append/0, tuple/0, list_copy/0, byte_list/0, copies/0, iolist/0,
         limit/0, collected/0]).

%% Runs F in a process of its own and returns how it ended, then builds a fresh list: the
%% answer comes only if the VM still serves.
victim(F, Opts) ->
    {Pid, Ref} = spawn_opt(F, [monitor | Opts]),
    Reason = receive {'DOWN', Ref, process, Pid, R} -> R after 5000 -> alive end,
    {Reason, length(grow([], 1000))}.

victim(F) -> victim(F, []).

%% The heap itself, grown a cell at a time by a list held live.
heap() -> victim(fun() -> grow([], 1 bsl 30) end).

%% A binary's construction: two halves joined.
binary() ->
    victim(fun() -> B = binary:copy(<<1>>, 600000), exit(byte_size(<<B/binary, B/binary>>)) end).

%% A binary's growth in place, a byte at a time, as a comprehension's accumulator grows.
append() -> victim(fun() -> exit(byte_size(app(<<>>, 2000000))) end).

app(B, 0) -> B;
app(B, N) -> app(<<B/binary, 7>>, N - 1).

%% A tuple of a size the process names.
tuple() -> victim(fun() -> exit(tuple_size(erlang:make_tuple(100000, a))) end).

%% A list's copy: `++` copies its left side.
list_copy() -> victim(fun() -> L = grow([], 20000), exit(length(L ++ L)) end).

%% A binary's bytes as a list: two cells a byte.
byte_list() -> victim(fun() -> exit(length(binary_to_list(binary:copy(<<1>>, 100000)))) end).

%% A binary of a size the process names.
copies() -> victim(fun() -> exit(byte_size(binary:copy(<<1>>, 2000000))) end).

%% An iolist flattened into one binary.
iolist() ->
    victim(fun() ->
        B = binary:copy(<<1>>, 600000),
        exit(byte_size(iolist_to_binary([B, B])))
    end).

%% Within what the allocator gives, but past the process's own limit: refused before it is made.
limit() ->
    victim(fun() -> exit(tuple_size(erlang:make_tuple(50000, a))) end,
           [{max_heap_size, #{size => 10000, kill => true}}]).

%% Past the limit only counting garbage: the heap is collected first, and the tuple is made.
collected() ->
    victim(fun() ->
        [binary:copy(<<N>>, 100000) || N <- [1, 2, 3, 4, 5, 6, 7, 8, 9]],
        exit(tuple_size(erlang:make_tuple(50000, a)))
    end, [{max_heap_size, #{size => 200000, kill => true, include_shared_binaries => true}}]).

grow(L, 0) -> L;
grow(L, N) -> grow([N | L], N - 1).
