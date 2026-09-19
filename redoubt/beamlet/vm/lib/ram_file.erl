%% Files in memory (file:open(Data, [ram | Modes])): OTP's ram_file API, without its C port
%% driver. The file is a small server process holding the contents and the position; it ends
%% when the file is closed or the process that opened it exits. Results are those OTP's
%% ram_file gives (eof, {error, ebadf} on a mode the file was not opened with, ...).
%% Embedded in the VM like beamlet_io, shadowing the kernel's module of the same name;
%% tools/build-lib regenerates the .beam.
-module(ram_file).
-export([open/2, close/1]).
-export([write/2, read/2, copy/3, pread/2, pread/3, pwrite/2, pwrite/3,
         position/2, truncate/1, datasync/1, sync/1]).
-export([get_size/1, get_file/1, advise/4, allocate/3, ipread_s32bu_p32bu/3]).

-include_lib("kernel/include/file.hrl").

-define(FD(Server), #file_descriptor{module = ?MODULE, data = Server}).

open(Data, Modes) when is_list(Modes) ->
    case modes(Modes, #{read => false, write => false, binary => false}) of
        {ok, #{read := false, write := false} = M} -> start(Data, M#{read := true});
        {ok, M} -> start(Data, M);
        Error -> Error
    end;
open(_, _) ->
    {error, badarg}.

modes([ram | T], M) -> modes(T, M);
modes([read | T], M) -> modes(T, M#{read := true});
modes([write | T], M) -> modes(T, M#{write := true});
modes([binary | T], M) -> modes(T, M#{binary := true});
modes([], M) -> {ok, M};
modes(_, _) -> {error, badarg}.

start(Data, Modes) ->
    try iolist_to_binary(Data) of
        Bin ->
            Owner = self(),
            Server = spawn(fun() -> init(Owner, Modes#{data => Bin, pos => 0}) end),
            {ok, ?FD(Server)}
    catch
        error:_ -> {error, badarg}
    end.

close(?FD(Server)) ->
    call(Server, close).

read(?FD(Server), Size) when is_integer(Size), Size >= 0 ->
    call(Server, {read, Size}).

write(?FD(Server), Bytes) ->
    with_binary(Bytes, fun(Bin) -> call(Server, {write, Bin}) end).

pread(?FD(Server), At, Size) when is_integer(At), is_integer(Size), Size >= 0 ->
    call(Server, {pread, At, Size});
pread(?FD(_), _, _) ->
    {error, badarg}.

pread(?FD(_) = Fd, List) when is_list(List) ->
    pread_list(Fd, List, []).

pread_list(_Fd, [], Acc) ->
    {ok, lists:reverse(Acc)};
pread_list(Fd, [{At, Size} | T], Acc) when is_integer(At), is_integer(Size), Size >= 0 ->
    case pread(Fd, At, Size) of
        {ok, Data} -> pread_list(Fd, T, [Data | Acc]);
        eof -> pread_list(Fd, T, [eof | Acc]);
        Error -> Error
    end;
pread_list(_, _, _) ->
    {error, badarg}.

pwrite(?FD(Server), At, Bytes) when is_integer(At) ->
    with_binary(Bytes, fun(Bin) -> call(Server, {pwrite, At, Bin}) end);
pwrite(?FD(_), _, _) ->
    {error, badarg}.

pwrite(?FD(_) = Fd, List) when is_list(List) ->
    pwrite_list(Fd, List, 0).

pwrite_list(_Fd, [], _N) ->
    ok;
pwrite_list(Fd, [{At, Bytes} | T], N) when is_integer(At) ->
    case pwrite(Fd, At, Bytes) of
        ok -> pwrite_list(Fd, T, N + 1);
        {error, badarg} = Error -> Error;
        {error, Reason} -> {error, {N, Reason}}
    end;
pwrite_list(_, _, _) ->
    {error, badarg}.

position(?FD(Server), Pos) ->
    case where(Pos) of
        {ok, From, Offset} -> call(Server, {position, From, Offset});
        Error -> Error
    end.

where(Pos) when is_integer(Pos) -> {ok, bof, Pos};
where(bof) -> {ok, bof, 0};
where(cur) -> {ok, cur, 0};
where(eof) -> {ok, eof, 0};
where({From, Offset}) when (From =:= bof orelse From =:= cur orelse From =:= eof), is_integer(Offset) ->
    {ok, From, Offset};
where(_) -> {error, badarg}.

truncate(?FD(Server)) -> call(Server, truncate).
datasync(?FD(Server)) -> call(Server, sync).
sync(?FD(Server)) -> call(Server, sync).
get_size(?FD(Server)) -> call(Server, size);
get_size(#file_descriptor{}) -> {error, enotsup}.
get_file(?FD(Server)) -> call(Server, get);
get_file(#file_descriptor{}) -> {error, enotsup}.

advise(?FD(Server), _Offset, _Length, Advice) ->
    case lists:member(Advice, [normal, random, sequential, will_need, dont_need, no_reuse]) of
        true -> call(Server, sync);
        false -> {error, einval}
    end;
advise(#file_descriptor{}, _, _, _) ->
    {error, enotsup}.

allocate(?FD(Server), _Offset, _Length) -> call(Server, sync);
allocate(#file_descriptor{}, _, _) -> {error, enotsup}.

copy(?FD(_) = Source, #file_descriptor{} = Dest, Length)
  when is_integer(Length), Length >= 0; is_atom(Length) ->
    file:copy_opened(Source, Dest, Length).

ipread_s32bu_p32bu(?FD(_) = Fd, Pos, MaxSize) ->
    file:ipread_s32bu_p32bu_int(Fd, Pos, MaxSize).

with_binary(Bytes, Fun) ->
    try iolist_to_binary(Bytes) of
        Bin -> Fun(Bin)
    catch
        error:Reason -> {error, Reason}
    end.

%% ---- the server ----

call(Server, Request) ->
    Ref = erlang:monitor(process, Server),
    Server ! {ram_file, self(), Ref, Request},
    receive
        {Ref, Reply} ->
            erlang:demonitor(Ref, [flush]),
            Reply;
        {'DOWN', Ref, process, Server, _} ->
            {error, einval}
    end.

init(Owner, S) ->
    erlang:monitor(process, Owner),
    loop(S).

loop(S) ->
    receive
        {ram_file, From, Ref, close} ->
            From ! {Ref, ok};
        {ram_file, From, Ref, Request} ->
            {Reply, S1} = handle(Request, S),
            From ! {Ref, Reply},
            loop(S1);
        {'DOWN', _, process, _, _} ->
            ok
    end.

handle({read, _}, #{read := false} = S) ->
    {{error, ebadf}, S};
handle({read, Size}, #{data := D, pos := P} = S) ->
    case take(D, P, Size) of
        <<>> when Size =/= 0 -> {eof, S};
        Bin -> {{ok, out(Bin, S)}, S#{pos := P + byte_size(Bin)}}
    end;
handle({pread, _, _}, #{read := false} = S) ->
    {{error, ebadf}, S};
handle({pread, At, _}, S) when At < 0 ->
    {{error, einval}, S};
handle({pread, At, Size}, #{data := D} = S) ->
    case take(D, At, Size) of
        <<>> when Size =/= 0 -> {eof, S};
        Bin -> {{ok, out(Bin, S)}, S}
    end;
handle({write, _}, #{write := false} = S) ->
    {{error, ebadf}, S};
handle({write, Bin}, #{data := D, pos := P} = S) ->
    {ok, S#{data := place(D, P, Bin), pos := P + byte_size(Bin)}};
handle({pwrite, _, _}, #{write := false} = S) ->
    {{error, ebadf}, S};
handle({pwrite, At, _}, S) when At < 0 ->
    {{error, einval}, S};
handle({pwrite, At, Bin}, #{data := D} = S) ->
    {ok, S#{data := place(D, At, Bin)}};
handle({position, From, Offset}, #{data := D, pos := P} = S) ->
    Base = case From of bof -> 0; cur -> P; eof -> byte_size(D) end,
    case Base + Offset of
        New when New < 0 -> {{error, einval}, S};
        New -> {{ok, New}, S#{pos := New}}
    end;
handle(truncate, #{write := false} = S) ->
    {{error, ebadf}, S};
handle(truncate, #{data := D, pos := P} = S) ->
    {ok, S#{data := take(D, 0, P)}};
handle(sync, S) ->
    {ok, S};
handle(size, #{data := D} = S) ->
    {{ok, byte_size(D)}, S};
handle(get, #{data := D} = S) ->
    {{ok, out(D, S)}, S}.

%% Up to Size bytes of D from At.
take(D, At, _Size) when At >= byte_size(D) -> <<>>;
take(D, At, Size) -> binary:part(D, At, min(Size, byte_size(D) - At)).

%% D with Bin written at At, zero-filled if At is past the end.
place(D, At, Bin) ->
    Size = byte_size(D),
    Head = case At > Size of
               true -> <<D/binary, 0:((At - Size) * 8)>>;
               false -> binary:part(D, 0, At)
           end,
    End = At + byte_size(Bin),
    Tail = case End < Size of
               true -> binary:part(D, End, Size - End);
               false -> <<>>
           end,
    <<Head/binary, Bin/binary, Tail/binary>>.

out(Bin, #{binary := true}) -> Bin;
out(Bin, #{binary := false}) -> binary_to_list(Bin).
