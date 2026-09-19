-module(garbage).
-export([start/0]).
%% Hostile arguments: the public crypto API called with a mix of valid algorithm names and
%% random or ill-typed values. Every call must return or raise; the NIFs must never crash the
%% VM. The oracle (OpenSSL) and beamlet only have to agree that all calls completed.
start() ->
    rand:seed(exsss, {1, 2, 3}),
    N = 20000,
    Done = lists:foldl(fun(_, Acc) -> call(), Acc + 1 end, 0, lists:seq(1, N)),
    {Done =:= N, done}.

call() ->
    Args = fun(K) -> [value() || _ <- lists:seq(1, K)] end,
    {F, A} = lists:nth(rand:uniform(14), [
        {hash, [alg(hash) | Args(1)]},
        {mac, [pick([hmac, poly1305, cmac, bogus]), alg(hash) | Args(2)]},
        {pbkdf2_hmac, [alg(hash), value(), value(), pick([1, 2, 0, -1, x]), pick([0, 1, 16, 32, -3])]},
        {crypto_one_time, [alg(cipher) | Args(4)]},
        {crypto_one_time, [alg(cipher) | Args(3)]},
        {crypto_one_time_aead, [alg(cipher) | Args(5)]},
        {crypto_one_time_aead, [alg(cipher), value(), value(), value(), value(), value(), pick([true, false])]},
        {crypto_init, [alg(cipher) | Args(3)]},
        {generate_key, [pick([eddh, eddsa, ecdh, dh, rsa]), pick([x25519, ed25519, secp256r1, secp384r1, x448, [7, 2]]), value()]},
        {compute_key, [pick([eddh, ecdh, dh]), value(), value(), pick([x25519, secp256r1, secp384r1, [23, 5]])]},
        {sign, [pick([rsa, ecdsa, eddsa, dss]), alg(hash), value(), value(), value()]},
        {verify, [pick([rsa, ecdsa, eddsa]), alg(hash), value(), value(), value(), value()]},
        {public_encrypt, [rsa, value(), value(), value()]},
        {mod_pow, [value(), value(), value()]}
    ]),
    try apply(crypto, F, A) catch _:_ -> ok end.

alg(hash) -> pick([md5, sha, sha256, sha512, sha3_256, none, bogus, 17]);
alg(cipher) -> pick([aes_128_cbc, aes_256_ctr, aes_128_ecb, aes_128_gcm, chacha20, chacha20_poly1305, bogus]).

pick(L) -> lists:nth(rand:uniform(length(L)), L).

%% Random values: binaries of awkward sizes, integers, atoms, lists, tuples, key lists.
value() ->
    case rand:uniform(12) of
        1 -> crypto_bin(rand:uniform(70) - 1);
        2 -> crypto_bin(pick([0, 1, 12, 15, 16, 17, 24, 31, 32, 33, 48, 64, 65, 128]));
        3 -> rand:uniform(1 bsl 70) - (1 bsl 69);
        4 -> pick([true, false, undefined, [], x25519, secp256r1]);
        5 -> [crypto_bin(rand:uniform(40)) || _ <- lists:seq(1, rand:uniform(4))];
        6 -> {crypto_bin(8), pick([secp256r1, x25519])};
        7 -> [{encrypt, pick([true, false, 3])}, {padding, pick([none, pkcs_padding, zero, bad])}];
        8 -> [{rsa_padding, pick([rsa_pkcs1_padding, rsa_pkcs1_pss_padding, rsa_no_padding])}];
        9 -> [crypto_bin(1), crypto_bin(128), crypto_bin(128)];
        10 -> <<1:3>>;
        11 -> [1, [2, <<3>>], 300];
        _ -> pick([0, -1, 16, 1 bsl 40])
    end.

%% Seeded, so a failure reproduces exactly.
crypto_bin(N) -> rand:bytes(N).
