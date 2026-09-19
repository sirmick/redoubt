-module(flat_size).
-export([start/0]).
%% erts_debug:flat_size/1 for each kind of term, as BEAM (64-bit, OTP 28) counts heap words.
start() ->
    Big = list_to_binary(lists:duplicate(65, 0)),
    <<_:8, Sub/binary>> = Big,
    [erts_debug:flat_size(T) || T <- [{}, 1, a, [], [1], "abc", {1, 2}, 1.5, 1 bsl 64, 1 bsl 128,
                                      -(1 bsl 64), list_to_binary([]), list_to_binary([1, 2, 3]),
                                      list_to_binary(lists:duplicate(64, 0)), Big, Sub,
                                      #{}, #{a => 1}, #{a => 1, b => 2}, self(), make_ref(),
                                      fun lists:map/2, {[1, {2}], #{k => [a]}}]].
