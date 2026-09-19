-module(maps_test).
-export([start/0]).
%% Map construction, update, matching and the maps module.
start() ->
    M = #{a => 1, b => 2},
    M2 = M#{c => 3},
    M3 = M2#{a := 10},
    #{b := B} = M3,
    {M, M2, M3, B, map_size(M3), maps:get(c, M3), maps:find(z, M3), lists:sort(maps:keys(M3)), lists:sort(maps:values(M3)),
     lists:sort(maps:to_list(M3)), maps:from_list([{x, 1}, {x, 2}]), maps:merge(M, #{b => 20, d => 4}),
     maps:remove(a, M), maps:is_key(a, M), maps:put(1, one, #{}), #{1 => a, 1.0 => b},
     maps:map(fun(_K, V) -> V * 2 end, M), lists:sort(maps:fold(fun(K, V, A) -> [{K, V} | A] end, [], M)),
     try M#{z := 1} catch error:E -> E end,
     try maps:get(z, M) catch error:E2 -> E2 end,
     case M of #{a := 1} -> matched; _ -> nope end}.
