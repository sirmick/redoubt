-module(hashes).
-export([start/0]).
%% Hashes, streaming hashes, HMAC, Poly1305, PBKDF2, hash_equals, info. The oracle is OTP's
%% crypto on OpenSSL.
start() ->
    Msg = <<"The quick brown fox jumps over the lazy dog">>,
    Algs = [md5, sha, sha224, sha256, sha384, sha512, sha3_224, sha3_256, sha3_384, sha3_512],
    Streamed = fun(A) ->
        S0 = crypto:hash_init(A),
        S1 = crypto:hash_update(S0, <<"The quick brown fox ">>),
        S2 = crypto:hash_update(S1, [<<"jumps over">>, " the lazy dog"]),
        _ = crypto:hash_update(S1, <<"ignored: states are immutable">>),
        crypto:hash_final(S2)
    end,
    MacState = crypto:mac_init(hmac, sha256, <<"key">>),
    MacState = crypto:mac_update(MacState, <<"The quick brown fox ">>),
    crypto:mac_update(MacState, <<"jumps over the lazy dog">>),
    {[crypto:hash(A, Msg) || A <- Algs],
     [crypto:hash(A, <<>>) || A <- Algs],
     [Streamed(A) =:= crypto:hash(A, Msg) || A <- Algs],
     [crypto:mac(hmac, A, <<"key">>, Msg) || A <- Algs],
     crypto:mac(hmac, sha256, binary:copy(<<"k">>, 200), Msg),
     crypto:macN(hmac, sha256, <<"key">>, Msg, 12),
     crypto:mac_final(MacState),
     crypto:mac(poly1305, binary:copy(<<1,2,3,4>>, 8), Msg),
     crypto:pbkdf2_hmac(sha256, <<"password">>, <<"salt">>, 4096, 32),
     crypto:pbkdf2_hmac(sha, <<"password">>, <<"salt">>, 2, 20),
     crypto:hash_equals(<<"abc">>, <<"abc">>), crypto:hash_equals(<<"abc">>, <<"abd">>),
     [crypto:hash_info(A) || A <- [sha, sha256, sha512, sha3_256]],
     try crypto:hash(nope, Msg) catch error:{Id, _, Text} -> {Id, Text} end,
     byte_size(crypto:strong_rand_bytes(33))}.
