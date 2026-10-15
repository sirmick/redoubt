-module(binary_compiled).
-export([start/0]).
%% A pattern compiled once (binary:compile_pattern/1) through match, matches and split: the same
%% answers as the uncompiled list, leftmost-longest, with a scope, and a forged compiled pattern
%% is badarg.
start() ->
    Hay = <<"ab abc abcd xabc">>,
    List = [<<"abc">>, <<"ab">>, <<"abcd">>, <<"x">>],
    One = binary:compile_pattern(<<"abc">>),
    Many = binary:compile_pattern(List),
    [tuple_size(One), element(1, One), tuple_size(Many), element(1, Many),
     is_reference(element(2, Many)),
     binary:match(Hay, One), binary:match(Hay, Many), binary:match(Hay, List),
     binary:match(Hay, Many, [{scope, {4, 12}}]), binary:match(Hay, Many, [{scope, {16, 0}}]),
     binary:matches(Hay, Many), binary:matches(Hay, List),
     binary:matches(Hay, Many, [{scope, {3, 9}}]),
     binary:split(Hay, Many), binary:split(Hay, Many, [global]), binary:split(Hay, List, [global]),
     binary:split(Hay, One, [global, {scope, {0, 6}}]),
     binary:match(<<"zzz">>, Many), binary:split(<<"zzz">>, Many, [global]),
     badarg(fun() -> binary:match(Hay, {bm, <<"abc">>}) end),
     badarg(fun() -> binary:match(Hay, {ac, make_ref()}) end),
     badarg(fun() -> binary:compile_pattern(Many) end),
     badarg(fun() -> binary:compile_pattern([<<"a">>, <<>>]) end)].
badarg(F) ->
    try F() catch error:badarg -> badarg end.
