%% OTP's zlib over the VM's stream NIFs: round trips in every format, streaming in pieces,
%% chunked safeInflate, and the errors for corrupt or unfinished streams.
-module(zlib_test).
-export([start/0]).

data() -> list_to_binary([integer_to_list(N) || N <- lists:seq(1, 3000)]).

start() ->
    D = data(),
    RoundTrips = [zlib:uncompress(zlib:compress(D)) =:= D,
                  zlib:unzip(zlib:zip(D)) =:= D,
                  zlib:gunzip(zlib:gzip(D)) =:= D,
                  zlib:uncompress(zlib:compress(<<>>)) =:= <<>>],
    Shrinks = byte_size(zlib:compress(D)) < byte_size(D) div 2,
    %% Streaming: deflate in three pieces, inflate one byte at a time.
    Z = zlib:open(),
    ok = zlib:deflateInit(Z, best_compression),
    C = iolist_to_binary([zlib:deflate(Z, binary:part(D, 0, 100)), zlib:deflate(Z, binary:part(D, 100, byte_size(D) - 100)), zlib:deflate(Z, <<>>, finish)]),
    ok = zlib:deflateEnd(Z),
    I = zlib:open(),
    ok = zlib:inflateInit(I),
    Out = iolist_to_binary([zlib:inflate(I, <<B>>) || <<B>> <= C]),
    ok = zlib:inflateEnd(I),
    zlib:close(Z), zlib:close(I),
    %% safeInflate hands out bounded chunks until finished.
    S = zlib:open(),
    ok = zlib:inflateInit(S),
    Chunks = safe(S, zlib:safeInflate(S, zlib:compress(D)), []),
    %% gzip auto-detection (window bits 47) of a zlib and a gzip stream.
    Auto = [begin A = zlib:open(), ok = zlib:inflateInit(A, 47), R = iolist_to_binary(zlib:inflate(A, X)), zlib:close(A), R =:= D end
            || X <- [zlib:compress(D), zlib:gzip(D)]],
    Errors = [catch zlib:uncompress(<<1, 2, 3, 4>>),
              catch zlib:gunzip(<<31, 139, 8, 0, 0, 0, 0, 0, 0, 255, 1, 2, 3>>),
              catch zlib:uncompress(binary:part(zlib:compress(D), 0, 20))],
    {RoundTrips, Shrinks, Out =:= D, iolist_to_binary(Chunks) =:= D, length(Chunks) > 1, Auto,
     [element(1, element(2, E)) || E <- Errors]}.

safe(S, {continue, Out}, Acc) -> safe(S, zlib:safeInflate(S, []), [Out | Acc]);
safe(S, {finished, Out}, Acc) -> ok = zlib:inflateEnd(S), lists:reverse([Out | Acc]).
