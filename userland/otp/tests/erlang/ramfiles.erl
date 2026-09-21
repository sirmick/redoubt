%% Files in memory: file:open(Data, [ram | Modes]) and the file operations on them.
-module(ramfiles).
-export([start/0]).

start() ->
    {ok, F} = file:open(<<"hello world">>, [ram, read, write, binary]),
    A = [file:read(F, 5), file:position(F, cur), file:read(F, 100), file:read(F, 1),
         file:pread(F, 6, 5), file:pread(F, 50, 5), file:pread(F, [{0, 2}, {20, 1}]),
         file:position(F, {eof, -5}), file:write(F, <<"WORLD!">>), file:position(F, cur),
         file:pwrite(F, 15, "xy"), file:pread(F, 0, 100), file:position(F, 5), file:truncate(F),
         ram_file:get_file(F), ram_file:get_size(F), file:position(F, {bof, -1}),
         file:sync(F), file:close(F)],
    {ok, L} = file:open("abc\ndef\n", [ram, read]),
    B = [file:read_line(L), file:read_line(L), file:read_line(L), file:write(L, "x"), file:close(L)],
    {ok, R} = file:open(["a", [<<"b">>], $c], [ram]),
    C = [file:read(R, 10), file:pwrite(R, 0, "z"), file:close(R)],
    D = [file:open(nonsense, [ram]), file:open("x", [ram, append])],
    {ok, Src} = file:open("FOO\n", [ram, binary]),
    {ok, Dst} = file:open(<<>>, [ram, write, binary]),
    E = [file:copy(Src, Dst), ram_file:get_file(Dst), file:close(Src), file:close(Dst)],
    {A, B, C, D, E}.
