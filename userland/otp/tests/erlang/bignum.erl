-module(bignum).
-export([start/0]).
%% Arbitrary-precision integers.
start() ->
    F = fact(50),
    {F, F div fact(48), F rem 1000007, -F, F band 16#FFFF, F bsr 200, integer_to_list(F, 36),
     2 bsl 128, (1 bsl 64) - 1, -(1 bsl 63), (1 bsl 63) - 1 + 1, pow(3, 100), F == F + 0.0,
     float(1 bsl 60), trunc(1.0e20), 1 bsl 64 > 1.0e19}.
fact(0) -> 1;
fact(N) -> N * fact(N - 1).
pow(_, 0) -> 1;
pow(B, E) -> B * pow(B, E - 1).
