-module(ciphers).
-export([start/0]).
%% Block and stream ciphers with every padding option, streaming updates, AEAD.
start() ->
    K16 = binary:copy(<<16#2b>>, 16), K24 = binary:copy(<<16#2c>>, 24), K32 = binary:copy(<<16#2d>>, 32),
    IV = <<0:64, 1:64>>, Nonce = <<7:96>>,
    Data = list_to_binary(lists:seq(1, 77)),
    Aligned = binary:part(Data, 0, 64),
    Pads = [undefined, none, pkcs_padding, zero],
    One = fun(C, K, I, D, Opts) -> try crypto:crypto_one_time(C, K, I, D, Opts) catch E:R -> {E, element(1, R)} end end,
    Round = fun(C, K, I, D, Pad) ->
        Enc = One(C, K, I, D, [{encrypt, true}, {padding, Pad}]),
        case is_binary(Enc) of
            true -> {Enc, One(C, K, I, Enc, [{encrypt, false}, {padding, Pad}])};
            false -> Enc
        end
    end,
    Streamed = fun(C, K, I, D) ->
        S = crypto:crypto_init(C, K, I, [{encrypt, true}, {padding, pkcs_padding}]),
        Parts = [crypto:crypto_update(S, P) || P <- [binary:part(D, 0, 5), binary:part(D, 5, 30), binary:part(D, 35, 42)]],
        Out = iolist_to_binary([Parts, crypto:crypto_final(S)]),
        {Out, crypto:crypto_get_data(S)}
    end,
    {[Round(aes_128_cbc, K16, IV, D, P) || D <- [Data, Aligned], P <- Pads],
     [Round(aes_256_cbc, K32, IV, Data, pkcs_padding), Round(aes_192_cbc, K24, IV, Aligned, none)],
     [Round(aes_128_ecb, K16, <<>>, D, P) || D <- [Data, Aligned], P <- [none, pkcs_padding]],
     [Round(C, K, IV, Data, undefined) || {C, K} <- [{aes_128_ctr, K16}, {aes_192_ctr, K24}, {aes_256_ctr, K32}]],
     Round(chacha20, K32, <<1:32/little, Nonce/binary>>, Data, undefined),
     Round(chacha20, K32, <<0:64, 5:64>>, Data, undefined),
     Streamed(aes_128_cbc, K16, IV, Data),
     crypto:crypto_one_time(aes_128_cbc, K16, IV, Data, [{encrypt, true}, {padding, pkcs_padding}])
         =:= element(1, Streamed(aes_128_cbc, K16, IV, Data)),
     aead(aes_128_gcm, K16, Nonce), aead(aes_256_gcm, K32, Nonce), aead(chacha20_poly1305, K32, Nonce),
     crypto:crypto_one_time_aead(aes_128_gcm, K16, Nonce, Data, <<"aad">>, 8, true),
     [crypto:cipher_info(C) || C <- [aes_128_cbc, aes_256_ctr, aes_128_gcm, chacha20, chacha20_poly1305, aes_192_ecb]]}.

aead(C, K, N) ->
    {Ct, Tag} = crypto:crypto_one_time_aead(C, K, N, <<"attack at dawn">>, <<"header">>, true),
    Bad = <<(binary:first(Tag) bxor 1), (binary:part(Tag, 1, byte_size(Tag) - 1))/binary>>,
    Enc = crypto:crypto_one_time_aead_init(C, K, 16, true),
    Dec = crypto:crypto_one_time_aead_init(C, K, 16, false),
    Both = crypto:crypto_one_time_aead(Enc, N, <<"attack at dawn">>, <<"header">>),
    {Ct, Tag,
     crypto:crypto_one_time_aead(C, K, N, Ct, <<"header">>, Tag, false),
     crypto:crypto_one_time_aead(C, K, N, Ct, <<"headers">>, Tag, false),
     crypto:crypto_one_time_aead(C, K, N, Ct, <<"header">>, Bad, false),
     Both, crypto:crypto_one_time_aead(Dec, N, Both, <<"header">>)}.
