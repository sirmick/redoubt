%% Lookaround at the edges of a pattern (as Elixir itself uses: `(?<!\\)\|` to split tables,
%% `^(?=.+)` to indent lines), checked around each match.
-module(re_edges).
-export([start/0]).

start() ->
    Opts = [global, {return, binary}],
    [re:split(<<"a|b\\|c|d">>, <<"(?<!\\\\)\\|">>, [{return, binary}]),
     re:replace(<<"one\n\ntwo\n">>, <<"^(?=.+)">>, <<"    ">>, [multiline | Opts]),
     re:run(<<"price: 30 EUR, 40 USD">>, <<"\\d+(?= USD)">>, [{capture, all, binary}]),
     re:run(<<"foobar foobaz">>, <<"foo(?!bar)">>, [{capture, all, index}]),
     re:run(<<"xay bay">>, <<"(?<=b)ay">>, [{capture, all, index}]),
     re:replace(<<"aXbXc">>, <<"(?<!a)X">>, <<"-">>, Opts),
     element(1, re:compile(<<"(?<=a+)b">>)),
     %% PCRE's `$` also matches before a final newline, unless dollar_endonly.
     [re:run(S, P, O) || {S, P, O} <- [{<<"abc\n">>, <<"c$">>, []}, {<<"abc\n">>, <<"c$">>, [dollar_endonly]},
                                      {<<"abc\n\n">>, <<"c$">>, []}, {<<"abc">>, <<"^abc$">>, []},
                                      {<<"a\\$">>, <<"\\$">>, []}]]].
