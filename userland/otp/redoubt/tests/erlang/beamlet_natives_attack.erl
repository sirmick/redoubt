%% beamlet-natives-attack (docs/userland/beamlet.md, "Natives"): what the host cannot show. One VM
%% writes a handle it holds, as term_to_binary writes it, to the home volume; a second VM, launched
%% after the first has ended, reads it back and offers it to every native that takes a handle, and
%% to each the hostile arguments of the host's table: each is refused by name, and the VM goes on.
%% One refusal is the kernel's, not the VM's: a child budget that adds a label, which only a
%% system-class caller may (class_denied). A third VM, in alice's labelled session, runs nothing:
%% the console carries no labels, so its write-open of /dev/cons is refused (consoled, R69) and
%% beamlet ends at its start, code 1, with no line; the tester's lines are the verdict.
-module(beamlet_natives_attack).
-export([write/0, read/0, labelled/0]).

write() ->
    {ok, Budget, <<>>} = redoubt:ns_lookup(<<"budget">>),
    say("wrote a handle: ~p", [file:write_file("/home/alice/handle.etf", term_to_binary(Budget))]),
    done.

read() ->
    {ok, Home, <<>>} = redoubt:ns_lookup(<<"/home/alice">>),
    {ok, Bytes} = file:read_file("/home/alice/handle.etf"),
    Theirs = binary_to_term(Bytes),
    say("another VM's handle decodes to a reference: ~p", [is_reference(Theirs)]),
    Child = #{pages => 1, processes => 1, weight => 1},
    Tries = [
        {call, fun() -> redoubt:call(Theirs, {[3, 0, 0, 0], nil, []}, 100) end},
        {send, fun() -> redoubt:send(Theirs, {[3, 0, 0, 0], nil, []}) end},
        {bind, fun() -> redoubt:bind(<<"/k">>, Theirs) end},
        {serve, fun() -> redoubt:serve(Theirs) end},
        {budget_usage, fun() -> redoubt:budget_usage(Theirs) end},
        {launch, fun() -> redoubt:launch(#{image => <<"x">>, budget => Theirs}) end},
        {path_with_nul, fun() -> redoubt:ns_lookup(<<"/home/al", 0, "ice">>) end},
        {path_with_dot_dot, fun() -> redoubt:ns_lookup(<<"/home/alice/../bob">>) end},
        {five_words, fun() -> redoubt:call(Home, {[1, 2, 3, 4, 5], nil, []}, 0) end},
        {seventeen_labels, fun() -> redoubt:budget_create(Child#{labels => lists:seq(1, 17)}) end},
        {wrong_kind, fun() -> redoubt:budget_usage(Home) end},
        {add_label, fun() -> redoubt:budget_create(Child#{labels => [7]}) end}
    ],
    [say("~p: ~p", [Name, try F() catch error:Reason -> {raised, Reason} end]) || {Name, F} <- Tries],
    say("the VM goes on: ~p", [length(redoubt:ns()) > 0]),
    done.

%% Never reached on the UART console: the case forbids its line.
labelled() ->
    say("labels: ~p", [redoubt:labels()]),
    done.

say(Format, Args) -> io:format("attack: " ++ Format ++ "~n", Args).
