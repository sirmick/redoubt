-module(lists_test).
-export([start/0]).
%% List BIFs and a good part of the real OTP `lists` module.
start() ->
    L = lists:seq(1, 10),
    {length(L), hd(L), tl([1]), L ++ [11], [1, 2, 3, 2, 1] -- [2, 1], lists:reverse(L),
     lists:map(fun(X) -> X * X end, L), lists:filter(fun(X) -> X rem 2 == 0 end, L),
     lists:foldl(fun(X, A) -> X + A end, 0, L), lists:sum(L), lists:max(L), lists:nth(3, L),
     lists:keyfind(b, 1, [{a, 1}, {b, 2}]), lists:keysort(2, [{a, 3}, {b, 1}, {c, 2}]),
     lists:member(5, L), lists:zip([1, 2], [a, b]), lists:flatten([1, [2, [3, [4]]], 5]),
     lists:sort(fun(A, B) -> A > B end, [3, 1, 2]), lists:split(3, L), lists:duplicate(3, x),
     lists:append([[1], [2, 3], []]), lists:last(L), lists:usort([c, a, b, a]),
     [X * 2 || X <- L, X > 5], [{X, Y} || X <- [1, 2], Y <- [a, b]]}.
