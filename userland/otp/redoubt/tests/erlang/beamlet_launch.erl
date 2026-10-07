%% beamlet-launch (docs/userland/native.md, "Launching from a session"): the shell's `exec` runs
%% /boot/beamlet-hello in a budget carved from the session's, with a connection of its own to the
%% session's console; its line reaches the console, and its end and its budget's use come back.
-module(beamlet_launch).
-export([start/0]).

start() ->
    Result = 'Elixir.Redoubt.Shell.Session':exec(<<"beamlet-hello">>, [<<"from">>, <<"the">>, <<"session">>]),
    % The whole result first, so a refusal is on the console before the match below fails on it.
    say("exec: ~p", [Result]),
    {Ending, #{pages := {Limit, _}, processes := Processes}} = Result,
    say("ended: ~p", [Ending]),
    say("its budget: ~p pages, processes ~p", [Limit, Processes]),
    done.

say(Format, Args) -> io:format("launch: " ++ Format ++ "~n", Args).
