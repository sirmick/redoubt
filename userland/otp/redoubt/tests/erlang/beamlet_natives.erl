%% beamlet-natives (docs/userland/beamlet.md, "Natives"): a session's VM, started by the cases'
%% tester in the steward's place, prints its namespace, binds its home volume at a second path
%% and writes through the bind what it reads back through the home, carves a child budget with a
%% deadline and reads its use before and after the deadline ends it, prints its label set, and
%% calls keyd through the generated client (keyd is the case's grant: the one typed server a
%% generated client can reach on the machine without the steward).
-module(beamlet_natives).
-export([start/0]).

start() ->
    say("ns: ~s", [lists:join(" ", [Path || {Path, _, _} <- redoubt:ns()])]),
    {ok, Home, <<>>} = redoubt:ns_lookup(<<"/home/alice">>),
    say("bind: ~p", [redoubt:bind(<<"/mnt">>, Home)]),
    say("write through the bind: ~p", [file:write_file("/mnt/notes.txt", <<"buy milk\n">>)]),
    say("read through the home: ~p", [file:read_file("/home/alice/notes.txt")]),
    Now = erlang:monotonic_time(microsecond),
    {ok, Child} = redoubt:budget_create(#{pages => 16, processes => 1, weight => 1, deadline => Now + 200000}),
    {ok, #{pages := {Pages, _}, processes := {Processes, _}}} = redoubt:budget_usage(Child),
    say("child: ~p pages, ~p process", [Pages, Processes]),
    receive after 500 -> ok end,
    say("child after its deadline: ~p", [redoubt:budget_usage(Child)]),
    say("labels: ~p", [redoubt:labels()]),
    {ok, Keyd, <<>>} = redoubt:ns_lookup(<<"keyd">>),
    case 'Elixir.Redoubt.Wire.Client.Keyd':public_key(Keyd) of
        {ok, #{key := Key}} -> say("keyd public key: ~p bytes", [byte_size(Key)]);
        Other -> say("keyd public key: ~p", [Other])
    end,
    done.

say(Format, Args) -> io:format("natives: " ++ Format ++ "~n", Args).
