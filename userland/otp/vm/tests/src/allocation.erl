%% Fixture for vm/tests/allocation.rs: each function has one process ask the VM for an
%% allocation, larger than the test's allocator gives or than the process's own limit, and
%% reports how that process ended and that the VM went on serving. Only BIFs are used: the
%% tests load no OTP modules. Rebuild: erlc +deterministic -o vm/tests/fixtures vm/tests/src/allocation.erl
-module(allocation).
-export([heap/0, binary/0, append/0, tuple/0, list_copy/0, byte_list/0, copies/0, iolist/0,
         limit/0, collected/0, map_keys/0, map_values/0, map_to_list/0, map_from_list/0,
         map_from_keys/0, ets_lookup/0, ets_take/0, ets_lookup_element/0, ets_match_object/0,
         characters/0, decoded/0, dict_keys/0, dict_get/0, dict_erase/0, integer_text/0,
         integer_bits/0, held/0, held_elements/0, held_chunk/0, held_tuple/0]).

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

%% The list a native makes of a 5,000-key map, made again and again and all of it kept: the call
%% that no longer fits meets the refusal.
map_keys() -> kept(fun maps:keys/1).

map_values() -> kept(fun maps:values/1).

map_to_list() -> kept(fun maps:to_list/1).

kept(F) -> victim(fun() -> M = map(5000, #{}), exit(length(again(fun() -> F(M) end, 20, []))) end).

%% A map of 7,000 pairs: the pairs and the map together are past what the allocator gives.
map_from_list() -> victim(fun() -> exit(map_size(maps:from_list(pairs(7000, [])))) end).

%% A map of 9,000 keys, likewise.
map_from_keys() -> victim(fun() -> exit(map_size(maps:from_keys(grow([], 9000), x))) end).

%% A table's ten objects under one key, each holding a 5,000-element list, copied out at once.
ets_lookup() -> victim(fun() -> exit(length(ets:lookup(table(), k))) end).

ets_take() -> victim(fun() -> exit(length(ets:take(table(), k))) end).

ets_lookup_element() -> victim(fun() -> exit(length(ets:lookup_element(table(), k, 3))) end).

%% What `ets:tab2list/1` calls.
ets_match_object() -> victim(fun() -> exit(length(ets:match_object(table(), '_'))) end).

%% 100,000 characters as a list: two cells each.
characters() ->
    victim(fun() -> exit(length(unicode:characters_to_list(binary:copy(<<"a">>, 100000), unicode))) end).

%% A term decoded from 40,000 bytes: a string of them, two cells each.
decoded() ->
    victim(fun() ->
        exit(length(binary_to_term(<<131, 107, 40000:16, (binary:copy(<<1>>, 40000))/binary>>)))
    end).

%% The process dictionary's 20,000 keys as lists: its keys, made twice and kept; its entries;
%% its entries as it is emptied.
dict_keys() -> victim(fun() -> dict(1, 20000), exit(length(again(fun() -> get_keys() end, 2, []))) end).

dict_get() -> victim(fun() -> dict(1, 20000), exit(length(get())) end).

dict_erase() -> victim(fun() -> dict(1, 20000), exit(length(erase())) end).

%% The 60,206 digits of a large bignum, two cells each: its width read from the dictionary, so
%% the compiler cannot make the digits a literal.
integer_text() ->
    victim(fun() ->
        put(bits, 200000),
        exit(length(integer_to_list(1 bsl get(bits))))
    end).

%% integer_to_list(1 bsl 16777000, 2): a bignum near the VM's 2^24 bits, 16 MiB of digits.
integer_bits() ->
    victim(fun() ->
        put(bits, 16777000),
        exit(length(integer_to_list(1 bsl get(bits), 2)))
    end).

%% A native's own copy of a list it reads, held aside: a 30,000-element list given as
%% demonitor's options. The test's allocator is told first that only 64 KiB more may be held.
held() ->
    victim(fun() ->
        L = grow([], 30000),
        erlang:display({pages, 64}),
        exit(catch erlang:demonitor(make_ref(), L))
    end).

%% lookup_element's copy of one element of each of 5,000 objects under one key, held aside
%% while the thread may hold only 64 KiB more.
held_elements() ->
    victim(fun() ->
        T = ets:new(t, [duplicate_bag]),
        objects(T, 5000),
        erlang:display({pages, 64}),
        exit(length(ets:lookup_element(T, k, 2)))
    end).

%% A match's results handed out 4,000 at a time, likewise.
held_chunk() ->
    victim(fun() ->
        T = ets:new(t, [duplicate_bag]),
        objects(T, 5000),
        erlang:display({pages, 64}),
        {Chunk, _} = ets:match_object(T, '_', 4000),
        exit(length(Chunk))
    end).

%% setelement's copy of a 30,000-element tuple, likewise.
held_tuple() ->
    victim(fun() ->
        T = erlang:make_tuple(30000, a),
        erlang:display({pages, 64}),
        exit(tuple_size(setelement(1, T, b)))
    end).

objects(_, 0) -> ok;
objects(T, N) -> ets:insert(T, {k, N}), objects(T, N - 1).

grow(L, 0) -> L;
grow(L, N) -> grow([N | L], N - 1).

again(_, 0, Acc) -> Acc;
again(F, N, Acc) -> again(F, N - 1, [F() | Acc]).

map(0, M) -> M;
map(N, M) -> map(N - 1, M#{N => N}).

pairs(0, L) -> L;
pairs(N, L) -> pairs(N - 1, [{N, N} | L]).

table() ->
    T = ets:new(t, [bag]),
    L = grow([], 5000),
    [ets:insert(T, {k, I, L}) || I <- [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]],
    T.

dict(I, N) when I > N -> ok;
dict(I, N) -> put(I, I), dict(I + 1, N).
