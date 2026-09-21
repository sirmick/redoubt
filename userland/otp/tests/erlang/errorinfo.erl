%% erlang:error/2,3: the first stack frame is the caller's, with the given arguments (or its
%% arity for `none`) and the error_info option in its location list.
-module(errorinfo).
-export([start/0, f/2, g/1]).

f(A, B) -> error(oops, [A, B], [{error_info, #{cause => #{position => 7}}}]).
g(X) -> error({bad, X}, none).

top(F) ->
    try F() catch C:R:S ->
        [{M, Fn, Args, Loc} | _] = S,
        {C, R, M, Fn, Args, [I || {error_info, _} = I <- Loc]}
    end.

start() ->
    {top(fun() -> f(1, 2) end), top(fun() -> g(x) end), top(fun() -> error(plain) end)}.
