-module(ets_test).
-export([start/0]).
%% ETS: table types, access, counters, traversal, match patterns and match specifications.
start() ->
    T = ets:new(t, [set, public]),
    true = ets:insert(T, [{a, 1}, {b, 2}, {c, 3}]),
    false = ets:insert_new(T, {a, 9}),
    true = ets:insert_new(T, {d, 4}),
    ets:insert(T, {a, 10}),
    Named = ets:new(named, [named_table, ordered_set]),
    ets:insert(named, [{3, three}, {1, one}, {2.0, two}]),
    B = ets:new(b, [bag]),
    ets:insert(B, [{k, 1}, {k, 2}, {k, 1}]),
    D = ets:new(d, [duplicate_bag]),
    ets:insert(D, [{k, 1}, {k, 1}]),
    C = ets:new(c, []),
    ets:insert(C, {hits, 0, 100}),
    {ets:lookup(T, a), ets:lookup(T, zz), ets:member(T, b), ets:lookup_element(T, c, 2),
     lists:sort(ets:tab2list(T)), ets:info(T, size), ets:info(T, type), Named,
     ets:first(named), ets:next(named, 1), ets:last(named), ets:lookup(named, 2), ets:tab2list(named),
     ets:lookup(B, k), ets:lookup(D, k), ets:info(B, size),
     ets:update_counter(C, hits, 1), ets:update_counter(C, hits, {3, -10}),
     ets:update_counter(C, hits, [{2, 5}, {3, 1}]), ets:update_counter(C, hits, {2, 100, 50, 0}),
     ets:update_counter(C, new, 7, {new, 0, 0}), ets:lookup(C, hits),
     ets:update_element(T, b, {2, twenty}), ets:lookup(T, b),
     lists:sort(ets:match(T, {'$1', '_'})), lists:sort(ets:match_object(T, {'_', 3})),
     lists:sort(ets:select(T, [{{'$1', '$2'}, [{is_integer, '$2'}, {'>', '$2', 3}], [{{'$2', '$1'}}]}])),
     ets:select_count(T, [{{'_', '$1'}, [{is_integer, '$1'}], [true]}]),
     ets:select_delete(T, [{{'$1', '_'}, [{'=:=', '$1', d}], [true]}]),
     lists:sort(ets:tab2list(T)), ets:take(T, c), ets:lookup(T, c),
     ets:foldl(fun({_, V}, Acc) when is_integer(V) -> V + Acc; (_, Acc) -> Acc end, 0, T),
     ets:delete(T, a), ets:lookup(T, a), ets:delete_all_objects(B), ets:info(B, size),
     ets:whereis(named) =/= undefined, ets:delete(named), ets:whereis(named), ets:info(named),
     try ets:lookup(named, 1) catch error:badarg -> badarg end,
     try ets:new(t2, [bogus]) catch error:badarg -> badarg end,
     access()}.

%% A protected table is readable by others but only its owner may write.
access() ->
    Self = self(),
    T = ets:new(p, [protected]),
    ets:insert(T, {k, v}),
    spawn(fun() ->
        R = {ets:lookup(T, k), try ets:insert(T, {k, x}) catch error:badarg -> denied end},
        Self ! {done, R}
    end),
    receive {done, R} -> R end.
