-module(tls).
-export([start/0]).
%% A TLS handshake and data exchange between two processes in one VM, over memsock. The
%% results are deterministic, so they compare with the real BEAM's.
start() ->
    {ok, _} = application:ensure_all_started(ssl),
    #{server_config := SOpts, client_config := COpts} =
        public_key:pkix_test_data(#{server_chain => #{root => [{key, {namedCurve, secp256r1}}, {digest, sha256}],
                                                      peer => [{key, {namedCurve, secp256r1}}, {digest, sha256}]},
                                    client_chain => #{root => [{key, {namedCurve, secp256r1}}, {digest, sha256}],
                                                      peer => [{key, {namedCurve, secp256r1}}, {digest, sha256}]}}),
    Cb = memsock:cb_info(),
    Suites = fun(Cipher) -> ssl:filter_cipher_suites(ssl:cipher_suites(all, 'tlsv1.3'), [{cipher, fun(C) -> C =:= Cipher end}]) end,
    #{server_config := RsaS, client_config := RsaC} =
        public_key:pkix_test_data(#{server_chain => #{root => [{key, {rsa, 2048, 65537}}, {digest, sha256}],
                                                      peer => [{key, {rsa, 2048, 65537}}, {digest, sha256}]},
                                    client_chain => #{root => [{key, {rsa, 2048, 65537}}, {digest, sha256}],
                                                      peer => [{key, {rsa, 2048, 65537}}, {digest, sha256}]}}),
    [run(Cb, SOpts, COpts, V, []) || V <- ['tlsv1.3', 'tlsv1.2']] ++
    [run(Cb, SOpts, COpts, 'tlsv1.3', [{ciphers, Suites(C)}]) || C <- [aes_128_gcm, chacha20_poly1305]] ++
    [run(Cb, SOpts, COpts, 'tlsv1.3', [{supported_groups, [G]}]) || G <- [x25519, secp256r1, secp384r1]] ++
    [run(Cb, RsaS, RsaC, V, []) || V <- ['tlsv1.3', 'tlsv1.2']].

run(Cb, SOpts, COpts, Version, Extra) ->
    {A, B} = memsock:pair(),
    Self = self(),
    Server = spawn_link(fun() ->
        ok = memsock:controlling_process(B, self()),
        {ok, S} = ssl:handshake(B, [{cb_info, Cb}, {versions, [Version]}, {active, false} | Extra ++ SOpts], 10000),
        {ok, Data} = ssl:recv(S, 0, 10000),
        ok = ssl:send(S, [<<"echo: ">>, Data]),
        Self ! {server, ssl:connection_information(S, [protocol, selected_cipher_suite])},
        receive stop -> ssl:close(S) end
    end),
    ok = memsock:controlling_process(B, Server),
    {ok, C} = ssl:connect(A, [{cb_info, Cb}, {versions, [Version]}, {verify, verify_peer}, {active, false},
                              {server_name_indication, disable} | Extra ++ COpts], 10000),
    ok = ssl:send(C, <<"hello over tls">>),
    {ok, Reply} = ssl:recv(C, 0, 10000),
    ServerInfo = receive {server, I} -> I after 10000 -> timeout end,
    ClientInfo = ssl:connection_information(C, [protocol, selected_cipher_suite, ecc]),
    Server ! stop,
    ssl:close(C),
    {iolist_to_binary(Reply), ClientInfo, ServerInfo}.
