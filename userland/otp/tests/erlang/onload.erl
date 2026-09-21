%% code:load_binary/3 runs the module's on_load function: `ok` keeps the module, anything else
%% (or an exception) unloads it and returns {error, on_load_failure}.
-module(onload).
-export([start/0]).

load(Name, InitBody) ->
    Src = io_lib:format("-module(~s).~n-on_load(init/0).~n-export([v/0]).~n"
                        "init() -> ~s.~nv() -> persistent_term:get(~s, none).~n",
                        [Name, InitBody, Name]),
    {ok, Tokens, _} = erl_scan:string(lists:flatten(Src)),
    Forms = split(Tokens, [], []),
    {ok, Mod, Bin} = compile:forms(Forms, [binary, return_errors]),
    R = code:load_binary(Mod, "nofile", Bin),
    {R, erlang:module_loaded(Mod)}.

split([{dot, _} = D | T], Cur, Acc) ->
    {ok, F} = erl_parse:parse_form(lists:reverse([D | Cur])),
    split(T, [], [F | Acc]);
split([H | T], Cur, Acc) -> split(T, [H | Cur], Acc);
split([], [], Acc) -> lists:reverse(Acc).

start() ->
    Good = load("onload_good", "persistent_term:put(onload_good, ran), ok"),
    GoodValue = onload_good:v(),
    Bad = load("onload_bad", "not_ok"),
    Crash = load("onload_crash", "error(boom)"),
    {Good, GoodValue, Bad, Crash}.
