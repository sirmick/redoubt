-module(binary_split_scope).
-export([start/0]).
%% binary:split/3 with a scope beside global, trim and trim_all: a separator outside the scope
%% does not split, the pieces are the whole binary's, and trimming drops empty pieces as it does
%% without a scope.
start() ->
    Hay = <<",,a,,b,,,c,,">>,
    Sep = <<",">>,
    Two = [<<",">>, <<",,">>],
    [binary:split(Hay, Sep, [{scope, {2, 8}}]),
     binary:split(Hay, Sep, [global, {scope, {2, 8}}]),
     binary:split(Hay, Sep, [global, trim, {scope, {2, 8}}]),
     binary:split(Hay, Sep, [global, trim_all, {scope, {2, 8}}]),
     binary:split(Hay, Sep, [global, trim, {scope, {0, 12}}]),
     binary:split(Hay, Sep, [global, trim_all]),
     binary:split(Hay, Sep, [global, trim, {scope, {10, 2}}]),
     binary:split(Hay, Sep, [global, trim_all, {scope, {10, 2}}]),
     binary:split(Hay, Sep, [trim, {scope, {12, 0}}]),
     binary:split(Hay, Sep, [global, trim_all, {scope, {12, 0}}]),
     binary:split(Hay, Two, [global, {scope, {2, 8}}]),
     binary:split(Hay, Two, [global, trim_all, {scope, {1, 11}}]),
     binary:split(Hay, Two, [global, trim, {scope, {12, -12}}]),
     binary:matches(Hay, Two, [{scope, {2, 8}}]),
     try binary:split(Hay, Sep, [global, {scope, {3, 10}}]) catch error:badarg -> badarg end].
