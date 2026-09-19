%% The console I/O server: the group leader of every process, registered as `user` (and a
%% second instance as `standard_error`). It speaks the Erlang I/O protocol, so io:format/2 and
%% Elixir's IO.puts/1 work unchanged, and writes through the platform console.
%%
%% Written in Erlang, not Rust, to keep the VM's trusted base small. The compiled .beam next
%% to this file is embedded in the VM; tools/build-lib regenerates it.
-module(beamlet_io).
-export([start/1]).

start(Name) ->
    register(Name, self()),
    loop().

loop() ->
    receive
        {io_request, From, ReplyAs, Request} ->
            From ! {io_reply, ReplyAs, request(Request)},
            loop();
        _Other ->
            loop()
    end.

request({put_chars, Encoding, Chars}) ->
    put_chars(Encoding, Chars);
request({put_chars, Encoding, M, F, A}) ->
    try apply(M, F, A) of
        Chars -> put_chars(Encoding, Chars)
    catch
        _:_ -> {error, format}
    end;
request({put_chars, Chars}) ->
    put_chars(latin1, Chars);
request({requests, Requests}) ->
    requests(Requests);
request({get_geometry, _}) ->
    {error, enotsup};
request(getopts) ->
    [{binary, false}, {encoding, unicode}];
request({setopts, _}) ->
    ok;
request({get_line, _Encoding, _Prompt}) ->
    eof;
request({get_chars, _Encoding, _Prompt, _N}) ->
    eof;
request({get_until, _Encoding, _Prompt, _M, _F, _A}) ->
    eof;
request(_) ->
    {error, request}.

requests([]) -> ok;
requests([R | Rs]) ->
    case request(R) of
        ok -> requests(Rs);
        Error -> Error
    end.

put_chars(Encoding, Chars) ->
    In = case Encoding of
        latin1 -> latin1;
        _ -> unicode
    end,
    case unicode:characters_to_binary(Chars, In) of
        Bin when is_binary(Bin) ->
            erlang:display_string(Bin),
            ok;
        _ ->
            {error, badarg}
    end.
