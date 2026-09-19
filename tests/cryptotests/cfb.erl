%% AES in CFB mode (128- and 8-bit feedback), in one go and in pieces, both ways; and ECB,
%% which has no IV, ignoring any given.
-module(cfb).
-export([start/0]).

start() ->
    Plain = list_to_binary(lists:seq(0, 99)),
    [begin
         K = binary:part(list_to_binary(lists:seq(1, 32)), 0, KeyLen),
         IV = list_to_binary(lists:seq(100, 115)),
         C = crypto:crypto_one_time(Cipher, K, IV, Plain, true),
         P = crypto:crypto_one_time(Cipher, K, IV, C, false),
         S = crypto:crypto_init(Cipher, K, IV, true),
         Pieces = [crypto:crypto_update(S, binary:part(Plain, At, Len)) || {At, Len} <- [{0, 7}, {7, 20}, {27, 1}, {28, 72}]],
         D = crypto:crypto_init(Cipher, K, IV, [{encrypt, false}]),
         Back = [crypto:crypto_update(D, binary:part(C, At, Len)) || {At, Len} <- [{0, 33}, {33, 67}]],
         {Cipher, C, P =:= Plain, iolist_to_binary(Pieces) =:= C, iolist_to_binary(Back) =:= Plain,
          crypto:crypto_final(S), crypto:cipher_info(Cipher)}
     end || {Cipher, KeyLen} <- [{aes_128_cfb128, 16}, {aes_192_cfb128, 24}, {aes_256_cfb128, 32},
                                 {aes_128_cfb8, 16}, {aes_192_cfb8, 24}, {aes_256_cfb8, 32}]]
    ++ [lists:member(aes_128_cfb128, proplists:get_value(ciphers, crypto:supports()))]
    ++ [crypto:crypto_one_time(aes_128_ecb, <<1:128>>, IV, <<3:128>>, false) || IV <- [<<2:128>>, <<1, 2>>, <<>>]].
