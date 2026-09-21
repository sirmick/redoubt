-module(floats).
-export([start/0]).
%% Float arithmetic (the float-register instructions) and printing.
start() ->
    X = 1.5, Y = 2.25,
    {X + Y, X * Y, X - Y, Y / X, -X, X * 3, 0.1 + 0.2, 1.0e300 * 10.0e-300, 3.141592653589793,
     1.0e100, 1.0e-100, 123456.789, 1/3, 2.0e-5, 100.0, 1000.0,
     try 1.0e308 * 10.0 catch error:badarith -> overflow end, math_ok(X)}.
math_ok(X) when is_float(X), X > 1.0 -> guard_ok.
