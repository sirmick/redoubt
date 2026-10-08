%% beamlet-serve (docs/userland/beamlet.md, "Natives": serve/1, reply/2): the VM serves the
%% endpoint the cases' tester handed it as `service`, which `beamlet-caller` calls twice. The first
%% request arrives with the caller's badge, account and labels and is answered; the second is never
%% answered, and the serve thread ends it at its deadline while the VM goes on.
-module(beamlet_serve).
-export([start/0]).

start() ->
    {ok, Service, <<>>} = redoubt:ns_lookup(<<"service">>),
    say("serve: ~p", [redoubt:serve(Service)]),
    {request, First, Badge, Account, Labels, {[41 | _], _, _}} = receive R1 -> R1 end,
    say("request: badge ~p, account ~p, labels ~p", [Badge, Account, Labels]),
    say("reply: ~p", [redoubt:reply(First, {[0, 42, 0, 0], nil, []})]),
    {request, _Held, _, _, _, {[42 | _], _, _}} = receive R2 -> R2 end,
    say("second request held, never answered", []),
    % Past the serve thread's deadline (5 s): the caller has its answer, and the VM still runs.
    receive after 6000 -> ok end,
    say("still serving after the deadline", []),
    done.

say(Format, Args) -> io:format("serve: " ++ Format ++ "~n", Args).
