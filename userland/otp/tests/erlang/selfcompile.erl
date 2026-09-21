-module(selfcompile).
-export([start/0]).
%% OTP's compiler running on the VM: source to .beam in memory, loaded and called.
start() ->
    Src = "-module(hello).\n-export([f/1]).\nf(X) -> {X, lists:reverse([1,2,3]), #{a => X}}.\n",
    {ok, Toks, _} = erl_scan:string(Src),
    Forms = split(Toks, [], []),
    T0 = erlang:monotonic_time(millisecond),
    R = compile:forms(Forms, [binary, return_errors]),
    T1 = erlang:monotonic_time(millisecond),
    case R of
        {ok, Mod, Bin} -> {module, Mod} = code:load_binary(Mod, "nofile", Bin), {Mod, is_integer(T1 - T0), Mod:f(42)};
        Other -> Other
    end.
split([{dot, _} = D | T], Acc, Out) -> {ok, F} = erl_parse:parse_form(lists:reverse([D | Acc])), split(T, [], [F | Out]);
split([H | T], Acc, Out) -> split(T, [H | Acc], Out);
split([], _, Out) -> lists:reverse(Out).
