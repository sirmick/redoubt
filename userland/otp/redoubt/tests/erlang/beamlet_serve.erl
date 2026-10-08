%% beamlet-serve (docs/userland/beamlet.md, "Natives": serve/1, reply/2): the VM serves the
%% endpoint the cases' tester handed it, which `beamlet-caller` calls three times. The first
%% request arrives with the caller's badge, account and labels and is answered; the second is never
%% answered, and the serve thread ends it at its deadline while the VM goes on; the third, made
%% after that deadline, carries the caller's two answers, which the VM says: every line the case
%% judges is this VM's, since two programs' consoles have no order between them.
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
    % The serve thread's deadline (5 s) answers the second request `malformed`; the caller then
    % reports both answers, so this request's arrival is the VM still serving past the deadline.
    {request, Third, _, _, _, {[43, Status1, Word1, Status2], _, _}} = receive R3 -> R3 end,
    say("caller: 41 answered ~p ~p, 42 answered ~p", [Status1, Word1, Status2]),
    say("reply: ~p", [redoubt:reply(Third, {[0, 0, 0, 0], nil, []})]),
    say("still serving after the deadline", []),
    done.

say(Format, Args) -> io:format("serve: " ++ Format ++ "~n", Args).
