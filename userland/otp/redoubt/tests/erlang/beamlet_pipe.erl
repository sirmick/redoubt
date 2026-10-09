%% The pipe cases (docs/userland/native.md, "Standard input and output, and pipes"): a session runs
%% pipelines of /boot/pipe-stage through Redoubt.Pipeline, and says what it saw. Every judged line is
%% the session's: what reached it through the last pipe, what its own pipes and volume hold, and
%% what its budget says about its children (docs/testbench.md, rule F). A stage's own words never
%% reach the console but as the session draws them.
-module(beamlet_pipe).
-export([carries/0, out_of_processes/0, never_reads/0, no_authority/0, interrupted/0]).

-define(PIPELINE, 'Elixir.Redoubt.Pipeline').
-define(STAGE, <<"pipe-stage">>).

%% Bytes go from stage to stage, the end of the stream follows a writer's end, a writer is held
%% while its pipe is full, and the session feeds the first stage and reads the last.
carries() ->
    % Evidence, not a verdict: what starting piped costs, held here across the pipelines below.
    say("piped started in ~p ms", [start_ms()]),
    pipeline("say|cat|count", [[<<"say">>, <<"hello">>], [<<"cat">>], [<<"count">>]], []),
    % 20000 bytes through a pipe of one page: the writer waits on the reader again and again.
    pipeline("gen|count", [[<<"gen">>, <<"20000">>], [<<"count">>]], []),
    pipeline("lines|cat", [[<<"cat">>]], [{input, [<<"a">>, <<"b">>]}]),
    % The reader ends early: the writer's next write is refused, or the writer is ended with the job.
    pipeline("yes|head", [[<<"yes">>], [<<"head">>, <<"2">>]], []),
    % Evidence, not a verdict: what piped's budget holds after these.
    {ok, #{pages := {Limit, Used}}} = 'Elixir.Redoubt.Pipes':usage(),
    say("piped's budget holds ~p of ~p pages", [Used, Limit]),
    % Its last holder lets go: piped's budget is destroyed, and the next pipeline starts another.
    ok = 'Elixir.Redoubt.Pipes':release(),
    say("piped after its last holder let go: ~p", ['Elixir.Redoubt.Pipes':usage()]),
    say("piped started again in ~p ms", [start_ms()]),
    ok = 'Elixir.Redoubt.Pipes':release(),
    done.

%% A session of 4 processes holds its VM, piped and two stages: a third is refused before any starts.
out_of_processes() ->
    pipeline("cat|cat|cat", [[<<"cat">>], [<<"cat">>], [<<"cat">>]], []),
    pipeline("cat|cat", [[<<"cat">>], [<<"cat">>]], []),
    done.

%% A stage that never reads, and another that never ends, hold the pipeline no longer than its
%% last stage: then every stage's budget is destroyed, and piped's with the last pipeline, so the
%% session's weight is back.
never_reads() ->
    Before = carved(),
    pipeline("yes|wait|say", [[<<"yes">>], [<<"wait">>], [<<"say">>, <<"done">>]], []),
    After = carved(),
    say("never reads: carved before ~p, after ~p, the same: ~p", [Before, After, Before =:= After]),
    done.

%% A stage reaches only its three streams: the attacker between two honest stages tries every way
%% out (pipe-stage's own docs), and what the session finds afterwards is what it put there.
no_authority() ->
    {ok, _, _} = 'Elixir.Redoubt.Pipes':open(1),
    ok = file:make_dir(<<"/dev/pipe/canary">>),
    {ok, Writer} = file:open(<<"/dev/pipe/canary/w">>, [append, binary, raw]),
    ok = file:write(Writer, <<"canary">>),
    pipeline("attack|cat", [[<<"attack">>], [<<"cat">>]], []),
    ok = file:close(Writer),
    {ok, Reader} = file:open(<<"/dev/pipe/canary/r">>, [read, binary, raw]),
    say("no authority: the canary holds ~p", [read_all(Reader, <<>>)]),
    say("no authority: a file the attacker named in the home: ~p", [file:read_file_info(<<"/home/alice/pwned">>)]),
    done.

%% A line that ran a pipeline ends while its stage runs: the stage's budget is destroyed with it,
%% and piped's, since no other pipeline holds it.
interrupted() ->
    Before = carved(),
    Line = spawn(fun() -> ?PIPELINE:run([{?STAGE, [<<"wait">>]}], []) end),
    say("interrupted: a stage runs: ~p", [until(fun() -> carved() > Before end)]),
    exit(Line, kill),
    say("interrupted: carved again as before: ~p", [until(fun() -> carved() =:= Before end)]),
    done.

pipeline(Name, Stages, Opts) ->
    case ?PIPELINE:run([{?STAGE, Args} || Args <- Stages], Opts) of
        {ok, #{output := Output, endings := Endings}} ->
            say("~s: ~p ~w", [Name, Output, Endings]);
        Error ->
            say("~s: ~p", [Name, Error])
    end.

%% How long starting piped takes, in milliseconds: none runs, and this process then holds it.
start_ms() ->
    {error, not_running} = 'Elixir.Redoubt.Pipes':usage(),
    Started = erlang:monotonic_time(millisecond),
    {ok, _, _} = 'Elixir.Redoubt.Pipes':open(1),
    erlang:monotonic_time(millisecond) - Started.

%% The weight the session's children hold: piped's while a pipeline holds it, and each stage's
%% while it runs.
carved() ->
    {ok, Own} = 'Elixir.Redoubt.Budget':own(),
    {ok, #{weight := {_, Carved}}} = 'Elixir.Redoubt.Budget':usage(Own),
    Carved.

%% Whether `Test` holds within 10 s, asked every 50 ms.
until(Test) -> until(Test, 200).
until(_Test, 0) -> false;
until(Test, N) ->
    case Test() of
        true -> true;
        false -> receive after 50 -> until(Test, N - 1) end
    end.

read_all(File, Acc) ->
    case file:read(File, 4096) of
        {ok, Data} -> read_all(File, <<Acc/binary, Data/binary>>);
        eof -> Acc;
        {error, _} = Error -> Error
    end.

say(Format, Args) -> io:format("pipe: " ++ Format ++ "~n", Args).
