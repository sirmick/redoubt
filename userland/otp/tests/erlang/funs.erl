-module(funs).
-export([start/0, double/1]).
%% Closures, external funs, apply and higher-order calls.
start() ->
    K = 10,
    Add = fun(X) -> X + K end,
    Fact = fun F(0) -> 1; F(N) -> N * F(N - 1) end,
    Compose = fun(F, G) -> fun(X) -> F(G(X)) end end,
    Ext = fun ?MODULE:double/1,
    {Add(5), Fact(10), (Compose(Add, Ext))(3), Ext(4), apply(?MODULE, double, [7]), apply(Add, [1]),
     erlang:apply(fun lists:reverse/1, [[1, 2]]), lists:map(fun erlang:abs/1, [-1, 2]),
     is_function(Add), is_function(Add, 1), is_function(Add, 2),
     try Add(1, 2) catch error:{badarity, _} -> badarity end,
     try (id(not_a_fun))(1) catch error:{badfun, F} -> {badfun, F} end}.
double(X) -> X * 2.
id(X) -> X.
