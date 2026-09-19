%% The console I/O server: the group leader of every process, registered as `user` (and a
%% second instance as `standard_error`). It speaks the Erlang I/O protocol, so io:format/2,
%% io:get_line/1, io:read/1 and Elixir's IO.puts/1 and IO.gets/1 work unchanged, and talks to
%% the platform console.
%%
%% Output is written at once. Input requests are served in order from a buffer; when the
%% buffer runs dry the server subscribes to console input (beamlet:console_subscribe/0) and
%% the VM sends it {beamlet_console, Bytes} as input arrives, then {beamlet_console, eof}.
%% Output requests are still served while an input request waits, as OTP's `user` does.
%%
%% Written in Erlang, not Rust, to keep the VM's trusted base small. The compiled .beam next
%% to this file is embedded in the VM; tools/build-lib regenerates it.
-module(beamlet_io).
-export([start/1]).

%% buf: input not yet consumed (UTF-8). eof: no more will come. subscribed: input is flowing.
%% pending: waiting input requests, oldest first, as {From, ReplyAs, Request, State} where
%% State is `new` (prompt not yet shown), `prompted`, or for get_until {cont, C, First}: the
%% collector's continuation, and whether no line has been fed to it yet.
-record(st, {buf = <<>>, eof = false, subscribed = false, binary = false, pending = []}).

start(Name) ->
    register(Name, self()),
    loop(#st{}).

loop(St) ->
    receive
        {io_request, From, ReplyAs, Request} ->
            case input_request(Request) of
                true ->
                    Pending = St#st.pending ++ [{From, ReplyAs, Request, new}],
                    loop(serve(St#st{pending = Pending}));
                false ->
                    {Reply, St1} = request(Request, St),
                    From ! {io_reply, ReplyAs, Reply},
                    loop(St1)
            end;
        {beamlet_console, eof} ->
            loop(serve(St#st{eof = true, subscribed = false}));
        {beamlet_console, Bytes} when is_binary(Bytes) ->
            Buf = St#st.buf,
            loop(serve(St#st{buf = <<Buf/binary, Bytes/binary>>}));
        _Other ->
            loop(St)
    end.

input_request({get_line, _, _}) -> true;
input_request({get_chars, _, _, _}) -> true;
input_request({get_until, _, _, _, _, _}) -> true;
input_request({get_password, _}) -> true;
input_request({get_line, _}) -> true;
input_request({get_chars, _, _}) -> true;
input_request({get_until, _, _, _, _}) -> true;
input_request(_) -> false.

%% ---- output and options ----

request({put_chars, Encoding, Chars}, St) ->
    {put_chars(Encoding, Chars), St};
request({put_chars, Encoding, M, F, A}, St) ->
    try apply(M, F, A) of
        Chars -> {put_chars(Encoding, Chars), St}
    catch
        _:_ -> {{error, format}, St}
    end;
request({put_chars, Chars}, St) ->
    {put_chars(latin1, Chars), St};
request({requests, Requests}, St) ->
    requests(Requests, St);
request({get_geometry, _}, St) ->
    {{error, enotsup}, St};
request(getopts, St) ->
    {[{binary, St#st.binary}, {encoding, unicode}, {echo, false}], St};
request({setopts, Opts}, St) when is_list(Opts) ->
    {ok, St#st{binary = setopts(Opts, St#st.binary)}};
request(_, St) ->
    {{error, request}, St}.

setopts([], B) -> B;
setopts([binary | T], _) -> setopts(T, true);
setopts([{binary, B} | T], _) when is_boolean(B) -> setopts(T, B);
setopts([list | T], _) -> setopts(T, false);
setopts([_ | T], B) -> setopts(T, B).

requests([], St) -> {ok, St};
requests([R | Rs], St) ->
    case request(R, St) of
        {ok, St1} -> requests(Rs, St1);
        Error -> Error
    end.

put_chars(Encoding, Chars) ->
    case unicode:characters_to_binary(Chars, in_encoding(Encoding)) of
        Bin when is_binary(Bin) ->
            erlang:display_string(Bin),
            ok;
        _ ->
            {error, badarg}
    end.

in_encoding(latin1) -> latin1;
in_encoding(_) -> unicode.

%% ---- input ----

%% Answer waiting input requests while the buffer allows; subscribe when it does not.
serve(#st{pending = []} = St) ->
    St;
serve(#st{pending = [{From, ReplyAs, Request, State0} | Rest]} = St) ->
    State = prompt(Request, State0),
    case attempt(Request, State, St) of
        {done, Reply, Buf} ->
            From ! {io_reply, ReplyAs, Reply},
            serve(St#st{buf = Buf, pending = Rest});
        {more, State1, Buf} ->
            St1 = St#st{buf = Buf, pending = [{From, ReplyAs, Request, State1} | Rest]},
            subscribe(St1)
    end.

subscribe(#st{subscribed = true} = St) -> St;
subscribe(#st{eof = true} = St) -> St;
subscribe(St) ->
    beamlet:console_subscribe(),
    St#st{subscribed = true}.

prompt(Request, new) ->
    show_prompt(prompt_of(Request)),
    prompted;
prompt(_, State) ->
    State.

show_prompt(none) -> ok;
show_prompt(Prompt) -> put_chars(unicode, io_lib:format_prompt(Prompt)).

prompt_of({get_line, _, P}) -> P;
prompt_of({get_chars, _, P, _}) -> P;
prompt_of({get_until, _, P, _, _, _}) -> P;
prompt_of({get_line, P}) -> P;
prompt_of({get_chars, P, _}) -> P;
prompt_of({get_until, P, _, _, _}) -> P;
prompt_of(_) -> none.

%% {done, Reply, RestOfBuffer}, or {more, State, RestOfBuffer} to wait for more input.
attempt({get_line, P}, State, St) -> attempt({get_line, latin1, P}, State, St);
attempt({get_password, Enc}, State, St) -> attempt({get_line, Enc, ""}, State, St);
attempt({get_line, Enc, _}, State, #st{buf = Buf, eof = Eof} = St) ->
    case binary:match(Buf, <<"\n">>) of
        {Pos, 1} ->
            <<Line:(Pos + 1)/binary, Rest/binary>> = Buf,
            {done, reply(Line, Enc, St), Rest};
        nomatch when Eof, Buf =:= <<>> ->
            {done, eof, <<>>};
        nomatch when Eof ->
            {done, reply(Buf, Enc, St), <<>>};
        nomatch ->
            {more, State, Buf}
    end;
attempt({get_chars, P, N}, State, St) -> attempt({get_chars, latin1, P, N}, State, St);
attempt({get_chars, Enc, _, N}, State, #st{buf = Buf, eof = Eof} = St) ->
    case take_chars(Buf, N) of
        {Chars, Rest} ->
            {done, reply(Chars, Enc, St), Rest};
        short when Eof, Buf =:= <<>> ->
            {done, eof, <<>>};
        short when Eof ->
            {done, reply(Buf, Enc, St), <<>>};
        short ->
            {more, State, Buf}
    end;
attempt({get_until, P, M, F, A}, State, St) -> attempt({get_until, latin1, P, M, F, A}, State, St);
attempt({get_until, _Enc, Prompt, M, F, A}, State, St) ->
    {Cont, First} = case State of {cont, C, F1} -> {C, F1}; _ -> {[], true} end,
    get_until(M, F, A, Cont, Prompt, First, St).

%% Feed the collector a line at a time, as OTP's servers do, showing the prompt again before
%% each line after the first (as OTP's `user` does). The collector gets characters whatever
%% the request's encoding: the console is UTF-8.
get_until(M, F, A, Cont, Prompt, First, #st{buf = Buf, eof = Eof} = St) ->
    case binary:match(Buf, <<"\n">>) of
        {Pos, 1} ->
            First orelse show_prompt(Prompt),
            <<Line:(Pos + 1)/binary, Rest/binary>> = Buf,
            case apply(M, F, [Cont, chars(Line) | A]) of
                {done, Result, RestChars} ->
                    {done, result(Result, St), <<(unicode:characters_to_binary(RestChars))/binary, Rest/binary>>};
                {more, Cont1} ->
                    get_until(M, F, A, Cont1, Prompt, false, St#st{buf = Rest})
            end;
        nomatch when Eof ->
            Data = case Buf of <<>> -> eof; _ -> chars(Buf) end,
            case apply(M, F, [Cont, Data | A]) of
                {done, Result, _} -> {done, result(Result, St), <<>>};
                {more, _} -> {done, eof, <<>>}
            end;
        nomatch ->
            {more, {cont, Cont, First}, Buf}
    end.

chars(Bin) ->
    case unicode:characters_to_list(Bin) of
        L when is_list(L) -> L;
        _ -> binary_to_list(Bin)
    end.

%% The first N characters of a UTF-8 buffer, or `short`.
take_chars(Buf, N) ->
    case chars(Buf) of
        L when length(L) >= N ->
            {Head, Tail} = lists:split(N, L),
            {unicode:characters_to_binary(Head), unicode:characters_to_binary(Tail)};
        _ ->
            short
    end.

%% Input as the request asked for it: a string, or a binary in binary mode.
reply(Bin, Enc, #st{binary = true}) ->
    case Enc of
        latin1 -> unicode:characters_to_binary(Bin, unicode, latin1);
        _ -> Bin
    end;
reply(Bin, _Enc, #st{binary = false}) ->
    chars(Bin).

result(R, #st{binary = true}) when is_list(R) ->
    case unicode:characters_to_binary(R) of
        B when is_binary(B) -> B;
        _ -> R
    end;
result(R, _) ->
    R.
