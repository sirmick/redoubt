%% erlang:phash2/1,2 must agree with BEAM bit for bit (for everything but pids and refs).
-module(phash).
-export([start/0]).

terms() ->
    [0, 1, -1, 255, 256, 134217727, 134217728, -134217728, -134217729,
     1 bsl 40, -(1 bsl 40), 1 bsl 59, 1 bsl 60, -(1 bsl 60), 1 bsl 63, 1 bsl 64, -(1 bsl 100),
     123456789012345678901234567890,
     0.0, -0.0, 1.5, -2.25e300, 3.141592653589793,
     a, abc, 'hello world', 'ÿ', 'é', '日本', true, [],
     "", "a", "ab", "abc", "abcd", "abcde", "abcdefghijklmnopqrstuvwxyz",
     [256], [1, 2, 300, 4], [a | b], [1 | 2], ["nested", [lists]], [[]],
     {}, {a}, {a, b, c}, {{}, [], <<>>},
     <<>>, <<1>>, <<"abcdefghijk">>, <<"abcdefghijkl">>, <<"abcdefghijklm">>,
     list_to_binary(lists:seq(0, 255)), <<1:1>>, <<5:3>>, <<1, 2, 3:4>>,
     #{}, #{a => 1}, #{a => 1, b => 2}, #{b => 2, a => 1}, #{[x] => {y}, 1.0 => 1},
     maps:from_list([{I, I * I} || I <- lists:seq(1, 40)]),
     fun lists:map/2, fun erlang:self/0,
     {deep, [#{k => [<<"v">>, {1, 2.0}]}]}].

start() ->
    Ts = terms(),
    {[erlang:phash2(T) || T <- Ts],
     [erlang:phash2(T, 1000) || T <- Ts],
     [erlang:phash2(T, 1 bsl 32) || T <- Ts],
     erlang:phash2(Ts),
     [try erlang:phash2(x, R) catch C:E -> {C, E} end || R <- [0, -1, (1 bsl 32) + 1, foo]]}.
