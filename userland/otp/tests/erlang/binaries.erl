-module(binaries).
-export([start/0]).
%% Binary construction and pattern matching.
start() ->
    B = <<1, 2, 3, 4, 5>>,
    <<H, Rest/binary>> = B,
    <<X:16, Y:16/little, _/binary>> = B,
    <<A:3, Bb:5, _/bits>> = <<255>>,
    Str = <<"hello">>,
    Name = <<"world">>,
    {H, Rest, X, Y, A, Bb, byte_size(B), bit_size(<<1:3>>), <<Str/binary, " ", Name/binary>>,
     <<1:1, 0:1, 1:6>>, <<-1:16/signed>>, <<256:16/little>>, <<1.5/float>>, <<16#12345678:32/big>>,
     binary_to_list(Str), list_to_binary([1, [2, <<3, 4>>], 5]), iolist_size([1, <<2, 3>>, [4]]),
     [C || <<C>> <= <<"abc">>], << <<(C + 1)>> || <<C>> <= <<"abc">> >>,
     binary_part(B, 1, 2), split_binary(B, 2), <<"é"/utf8>>, parse(<<3, "abc", "rest">>),
     case B of <<1, 2, _/binary>> -> prefix; _ -> no end,
     try <<X:(-1)>> catch error:E -> E end}.
parse(<<N, S:N/binary, R/binary>>) -> {S, R}.
