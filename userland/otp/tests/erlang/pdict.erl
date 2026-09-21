-module(pdict).
-export([start/0]).
%% The process dictionary.
start() ->
    undefined = put(a, 1),
    1 = put(a, 2),
    put(b, 3),
    {get(a), get(b), get(zzz), lists:sort(get()), erase(a), get(a)}.
