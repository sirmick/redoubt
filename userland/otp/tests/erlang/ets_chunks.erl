%% Chunked ETS traversals (select/3, select_reverse/3, match/3, match_object/3 and their
%% continuations): the chunks come out in the same order as on BEAM.
-module(ets_chunks).
-export([start/0]).

drain('$end_of_table', Acc) -> lists:reverse(Acc);
drain({Chunk, Cont}, Acc) -> drain(ets:select(Cont), [Chunk | Acc]).

drain_m('$end_of_table', Acc) -> lists:reverse(Acc);
drain_m({Chunk, Cont}, Acc) -> drain_m(ets:match(Cont), [Chunk | Acc]).

start() ->
    B = ets:new(b, [bag]), O = ets:new(o, [ordered_set]),
    [ets:insert(B, {k, V}) || V <- lists:seq(1, 5)],
    [ets:insert(O, {K, K * K}) || K <- lists:seq(1, 7)],
    All = [{{'$1', '$2'}, [], ['$2']}],
    {drain(ets:select(B, All, 2), []),
     drain(ets:select(O, All, 3), []),
     drain(ets:select_reverse(O, All, 3), []),
     drain_m(ets:match(O, {'$1', '_'}, 4), []),
     element(1, ets:match_object(B, {k, '_'}, 1)),
     ets:select(ets:new(e, [set]), All, 1)}.
