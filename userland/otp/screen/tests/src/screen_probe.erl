%% Fixture for screen/tests/natives.rs: the screen buffer's natives, module redoubt_screen, as a
%% screen program calls them. Only natives are used: the tests load no OTP modules.
%% Rebuild: erlc +deterministic -o screen/tests/fixtures screen/tests/src/screen_probe.erl
-module(screen_probe).
-export([drawn/0, control/0, other_owner/0, fifth/0, freed/0, big/0, grown/0, styles/0, read/0]).

-define(PLAIN, {reset, reset, 0}).
-define(LIMIT, #{size => 100000, kill => true, error_logger => false}).

%% Text and a fill drawn, then the frame's bytes, for the test to decode.
drawn() ->
    B = redoubt_screen:new(10, 2),
    Written = redoubt_screen:put(B, 1, 0, [<<"h">>, <<"i">>], {{indexed, 2}, reset, 1}),
    ok = redoubt_screen:fill(B, {0, 1, 3, 1}, <<"-">>, ?PLAIN),
    ok = redoubt_screen:plot(B, {9, 1, 1, 1}, <<255>>, ?PLAIN),
    {Written, redoubt_screen:diff(B), redoubt_screen:diff(B)}.

%% A control character is badarg, from put and from fill.
control() ->
    B = redoubt_screen:new(10, 1),
    {reason(fun() -> redoubt_screen:put(B, 0, 0, [<<"a">>, <<27, "[2J">>], ?PLAIN) end),
     reason(fun() -> redoubt_screen:fill(B, {0, 0, 1, 1}, <<16#9b/utf8>>, ?PLAIN) end),
     redoubt_screen:diff(B)}.

%% Only the process that made a buffer may draw into it.
other_owner() ->
    B = redoubt_screen:new(4, 1),
    Self = self(),
    spawn(fun() -> Self ! {other, reason(fun() -> redoubt_screen:put(B, 0, 0, [<<"x">>], ?PLAIN) end)} end),
    receive {other, R} -> R end.

%% Four buffers, and a fifth is a system limit.
fifth() ->
    Held = [redoubt_screen:new(2, 2) || _ <- [1, 2, 3, 4]],
    {length(Held), reason(fun() -> redoubt_screen:new(2, 2) end)}.

%% A buffer nothing holds any more is given back: a fifth may then be made.
freed() ->
    _ = [redoubt_screen:new(2, 2) || _ <- [1, 2, 3, 4]],
    erlang:garbage_collect(),
    B = redoubt_screen:new(2, 2),
    is_reference(B).

%% A large buffer counts toward its owner's heap limit, from new and from resize.
big() ->
    limited(fun() ->
                    B = redoubt_screen:new(256, 256),
                    spin(1000),
                    B
            end).
grown() ->
    limited(fun() ->
                    B = redoubt_screen:new(2, 2),
                    ok = redoubt_screen:resize(B, 1024, 64),
                    spin(1000),
                    B
            end).

%% Styles the natives refuse.
styles() ->
    B = redoubt_screen:new(2, 1),
    [reason(fun() -> redoubt_screen:put(B, 0, 0, [<<"x">>], S) end)
     || S <- [{red, reset, 0}, {{indexed, 256}, reset, 0}, {reset, reset, 1024}, {reset, reset}]].

%% A program's frame read in: stopped at its joined symbol, gone on with after it, then frames
%% of another size, refused, gone on with from elsewhere, not a binary, and not the caller's.
read() ->
    B = redoubt_screen:new(4, 1),
    F = frame(4, 1, 0, [cell(0, <<"a">>), cell(1, <<"e", 16#301/utf8>>), cell(2, <<"界"/utf8>>)]),
    {joined, Next, 1, 0, Joined, Style} = redoubt_screen:frame(B, F, 0),
    1 = redoubt_screen:put(B, 1, 0, [Joined], Style),
    Done = redoubt_screen:frame(B, F, Next),
    Self = self(),
    spawn(fun() -> Self ! {other, reason(fun() -> redoubt_screen:frame(B, F, 0) end)} end),
    Other = receive {other, R} -> R end,
    {Joined, Style, Done, redoubt_screen:diff(B),
     redoubt_screen:frame(B, frame(5, 1, 1, []), 0),
     redoubt_screen:frame(B, frame(4, 1, 0, [cell(0, <<"a">>), cell(0, <<"b">>)]), 0),
     redoubt_screen:frame(B, frame(4, 1, 0, [cell(3, <<"界"/utf8>>)]), 0),
     reason(fun() -> redoubt_screen:frame(B, F, Next) end),
     reason(fun() -> redoubt_screen:frame(B, [F], 0) end),
     Other}.

frame(W, H, Flags, Cells) ->
    <<1, Flags, W:16/little, H:16/little, (length(Cells)):32/little, (iolist_to_binary(Cells))/binary>>.

cell(X, S) -> <<X:16/little, 0:16, (byte_size(S)), S/binary, 1, 1, 0, 0, 0:32, 1:16/little>>.

limited(F) ->
    {Pid, Ref} = spawn_opt(F, [monitor, {max_heap_size, ?LIMIT}]),
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason end.

reason(F) ->
    try F() of V -> {ok, V} catch error:R -> R end.

spin(0) -> ok;
spin(N) -> erlang:yield(), spin(N - 1).
