%% The code every port runs (see vm/src/bif/port.rs). A port is a process marked as a port, so
%% self() here is the port. The VM starts the program and sends what it does as
%% {'$beamlet_program', {data, Bytes} | eof | {exit_status, N}}; this turns that into the
%% messages a port sends its connected process, framed as the port was opened (stream, lines
%% or packets), and handles the messages a port accepts ({Pid, {command, Data}}, {Pid, close},
%% {Pid, {connect, New}}). Writing, closing and port_info are the VM's (erlang:port_command/2,
%% erlang:port_close/1).
%% Embedded in the VM like beamlet_io; tools/build-lib regenerates the .beam.
-module(beamlet_port).
-export([init/1]).

init(Settings) ->
    %% An exit signal from a linked process ends a port unless it is `normal` (and any signal
    %% from the connected process ends it): so exits are trapped and handled below.
    process_flag(trap_exit, true),
    loop(Settings#{buffer => <<>>}).

loop(S) ->
    receive
        {'$beamlet_program', Event} -> loop(program(Event, S));
        {From, {command, Data}} when is_pid(From) ->
            check_owner(From),
            try erlang:port_command(self(), Data) catch error:badarg -> exit(badsig) end,
            loop(S);
        {From, close} when is_pid(From) ->
            check_owner(From),
            From ! {self(), closed},
            erlang:port_close(self()),
            exit(normal);
        {From, {connect, New}} when is_pid(From), is_pid(New) ->
            check_owner(From),
            try erlang:port_connect(self(), New) catch error:badarg -> exit(badsig) end,
            unlink(From),
            From ! {self(), connected},
            loop(S);
        {'EXIT', From, Reason} ->
            case owner() of
                From -> exit(Reason);
                _ when Reason =:= normal -> loop(S);
                _ -> exit(Reason)
            end;
        _ ->
            exit(badsig)
    end.

owner() ->
    case erlang:port_info(self(), connected) of
        {connected, Owner} -> Owner;
        %% Closed (port_close/1) while this was still running on another scheduler: the
        %% signal that ends it is on its way; stop now.
        undefined -> exit(normal)
    end.

%% Only the connected process may command a port; anything else is a bad signal.
check_owner(From) ->
    case owner() of
        From -> ok;
        _ -> exit(badsig)
    end.

deliver(Msg) ->
    owner() ! {self(), Msg}.

program({data, Bytes}, #{framing := stream} = S) ->
    deliver({data, out(Bytes, S)}),
    S;
program({data, Bytes}, #{framing := {packet, N}, buffer := Buf} = S) ->
    S#{buffer := packets(<<Buf/binary, Bytes/binary>>, N, S)};
program({data, Bytes}, #{framing := {line, L}, buffer := Buf} = S) ->
    S#{buffer := lines(<<Buf/binary, Bytes/binary>>, L, S)};
program(eof, #{framing := {line, _}, eof := false, exit_status := true} = S) ->
    %% As BEAM: a last unfinished line comes when the port closes, after the exit status.
    S;
program(eof, #{framing := {line, _}, buffer := Buf} = S) when Buf =/= <<>> ->
    deliver({data, {noeol, out(Buf, S)}}),
    program(eof, S#{buffer := <<>>});
program(eof, #{eof := true} = S) ->
    deliver(eof),
    S;
program(eof, #{exit_status := true} = S) ->
    %% The port closes once the exit status has been passed on.
    S;
program(eof, _S) ->
    erlang:port_close(self()),
    exit(normal);
program({exit_status, N}, #{exit_status := true, eof := false, buffer := Buf} = S) ->
    deliver({exit_status, N}),
    case S of
        #{framing := {line, _}} when Buf =/= <<>> -> deliver({data, {noeol, out(Buf, S)}});
        _ -> ok
    end,
    erlang:port_close(self()),
    exit(normal);
program({exit_status, N}, #{exit_status := true} = S) ->
    deliver({exit_status, N}),
    S;
program({exit_status, _}, S) ->
    S.

out(Bytes, #{binary := true}) -> Bytes;
out(Bytes, #{binary := false}) -> binary_to_list(Bytes).

%% Pass on every whole packet; keep the rest.
packets(Buf, N, S) ->
    case Buf of
        <<Len:N/unit:8, Packet:Len/binary, Rest/binary>> ->
            deliver({data, out(Packet, S)}),
            packets(Rest, N, S);
        _ ->
            Buf
    end.

%% Pass on every whole line ({eol, Line}), and lines longer than L in pieces ({noeol, Part});
%% keep the rest.
lines(Buf, L, S) ->
    case binary:match(Buf, <<"\n">>) of
        {Pos, 1} when Pos =< L ->
            <<Line:Pos/binary, $\n, Rest/binary>> = Buf,
            deliver({data, {eol, out(Line, S)}}),
            lines(Rest, L, S);
        _ when byte_size(Buf) > L ->
            <<Part:L/binary, Rest/binary>> = Buf,
            deliver({data, {noeol, out(Part, S)}}),
            lines(Rest, L, S);
        _ ->
            Buf
    end.
