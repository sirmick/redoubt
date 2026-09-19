-module(arith).
-export([start/0]).
%% Integer and float arithmetic, including overflow into bignums and back.
start() ->
    Big = 1 bsl 100,
    [1 + 2, 7 - 10, 6 * 7, 7 div 2, -7 div 2, 7 rem -2, -7 rem 2,
     9223372036854775807 + 1, -9223372036854775808 - 1,
     Big, Big * Big, Big div 3, Big rem 7, -Big, (Big + 1) - Big,
     fact(30), 1.5 + 2, 10 / 4, 2.0 * 3, -0.0, abs(-5), abs(-2.5),
     b_and(12, 10), 12 bor 10, 12 bxor 10, bnot 5, 1 bsl 70 bsr 69, -16 bsr 2, -1 bsr 100,
     trunc(2.7), round(2.5), round(-2.5), floor(-1.5), ceil(1.2), float(3),
     max(1, 1.0), min(2, 1.0), 1 == 1.0, 1 =:= 1.0, 1 < 1.0, 1.0 < 1].
b_and(A, B) -> A band B.
fact(0) -> 1;
fact(N) -> N * fact(N - 1).
