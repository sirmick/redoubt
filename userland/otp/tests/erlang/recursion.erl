-module(recursion).
-export([start/0]).
%% Deep body recursion, long tail recursion and big lists: preemption and stack growth.
start() ->
    L = lists:seq(1, 100000),
    {len(L), sum(L, 0), length(lists:reverse(L)), fib(20)}.
len([]) -> 0;
len([_ | T]) -> 1 + len(T).
sum([], A) -> A;
sum([H | T], A) -> sum(T, A + H).
fib(0) -> 0;
fib(1) -> 1;
fib(N) -> fib(N - 1) + fib(N - 2).
