-module(pubkey).
-export([start/0]).
%% Key agreement and signatures with fixed keys (deterministic results the oracle can match),
%% and sign/verify round trips where signing is randomized.
start() ->
    Msg = <<"sign me">>,
    %% X25519 (RFC 7748 section 6.1 keys).
    APriv = hex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a"),
    BPriv = hex("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb"),
    {APub, APriv} = crypto:generate_key(eddh, x25519, APriv),
    {BPub, BPriv} = crypto:generate_key(eddh, x25519, BPriv),
    X = {APub, crypto:compute_key(eddh, BPub, APriv, x25519), crypto:compute_key(eddh, APub, BPriv, x25519)},
    %% Ed25519 (RFC 8032 test 1): deterministic signatures.
    EdPriv = hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"),
    {EdPub, EdPriv} = crypto:generate_key(eddsa, ed25519, EdPriv),
    EdSig = crypto:sign(eddsa, none, <<>>, [EdPriv, ed25519]),
    Ed = {EdPub, EdSig, crypto:verify(eddsa, none, <<>>, EdSig, [EdPub, ed25519]),
          crypto:verify(eddsa, none, <<"x">>, EdSig, [EdPub, ed25519])},
    %% ECDH and ECDSA on P-256 and P-384 with fixed private keys.
    Ec = [ec(Curve, Msg) || Curve <- [secp256r1, secp384r1]],
    %% RSA: fixed key, PKCS #1 v1.5 (deterministic) and PSS (randomized) signatures.
    {RsaPub, RsaPriv} = rsa_key(),
    Pk1 = crypto:sign(rsa, sha256, Msg, RsaPriv),
    Pss = crypto:sign(rsa, sha256, Msg, RsaPriv, [{rsa_padding, rsa_pkcs1_pss_padding}]),
    Rsa = {Pk1, crypto:verify(rsa, sha256, Msg, Pk1, RsaPub), crypto:verify(rsa, sha, Msg, Pk1, RsaPub),
           crypto:verify(rsa, sha256, Msg, Pss, RsaPub, [{rsa_padding, rsa_pkcs1_pss_padding}]),
           crypto:verify(rsa, sha256, <<"other">>, Pss, RsaPub, [{rsa_padding, rsa_pkcs1_pss_padding}]),
           crypto:sign(rsa, sha, {digest, crypto:hash(sha, Msg)}, RsaPriv) =:= crypto:sign(rsa, sha, Msg, RsaPriv),
           crypto:private_decrypt(rsa, crypto:public_encrypt(rsa, <<"secret">>, RsaPub, []), RsaPriv, []),
           crypto:public_decrypt(rsa, crypto:private_encrypt(rsa, <<"hello">>, RsaPriv, []), RsaPub, []),
           crypto:private_encrypt(rsa, <<"hello">>, RsaPriv, [])},
    %% Finite-field DH over the RFC 3526 group 14 prime: both sides agree. (OpenSSL ignores a
    %% given private key here, so keys are random and only agreement is compared.)
    P = crypto:bytes_to_integer(hex(modp2048())), G = 2,
    {DA, PA} = crypto:generate_key(dh, [P, G]),
    {DB, PB} = crypto:generate_key(dh, [P, G]),
    Dh = {byte_size(DA) > 200, crypto:compute_key(dh, DB, PA, [P, G]) =:= crypto:compute_key(dh, DA, PB, [P, G]),
          crypto:mod_pow(3, 1000, 1000007), crypto:mod_pow(<<2>>, <<10>>, <<1000:16>>)},
    %% Generated keys work (random, so only their shape and use are compared).
    {GP, GS} = crypto:generate_key(ecdh, secp256r1),
    {GP2, GS2} = crypto:generate_key(eddh, x25519),
    Gen = {byte_size(GP), byte_size(GS), byte_size(GP2), byte_size(GS2),
           crypto:verify(ecdsa, sha256, Msg, crypto:sign(ecdsa, sha256, Msg, [GS, secp256r1]), [GP, secp256r1])},
    {X, Ed, Ec, Rsa, Dh, Gen, crypto:privkey_to_pubkey(rsa, RsaPriv) =:= RsaPub}.

ec(Curve, Msg) ->
    Priv = <<1:256>>,
    {Pub, Priv2} = crypto:generate_key(ecdh, Curve, Priv),
    {Pub2, _} = crypto:generate_key(ecdh, Curve, <<2:256>>),
    Shared = crypto:compute_key(ecdh, Pub2, Priv, Curve),
    Sig = crypto:sign(ecdsa, sha256, Msg, [Priv2, Curve]),
    {Pub, Priv2, Shared, crypto:verify(ecdsa, sha256, Msg, Sig, [Pub, Curve]),
     crypto:verify(ecdsa, sha256, <<"other">>, Sig, [Pub, Curve])}.

%% A fixed 1024-bit RSA key (test only), generated once with OTP 28 and pasted here.
rsa_key() ->
    {E, N, D} = {65537, 131893630999216245271214587402062326407921447429914807228651437668399420914182594974898028873760985640349880580891775988186989168194904659682581812646690287881597716167264621773576934386538856824765404956418311076280612085542214553339764166114792664191988183462665612862850135170573531420708666651007893174251, 130160863001133861470025705400243877851575866060045013844397216574475501579957482225254511427733425504299386246696621813814817758551255836697295593553829738511998434313594303285386952373537187936752117392450810888914017851847927438463607846719975121427123331946855376411469929193321749226243281009206943151329},
    {[E, N], [E, N, D]}.

modp2048() ->
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3BE39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6955817183995497CEA956AE515D2261898FA051015728E5A8AACAA68FFFFFFFFFFFFFFFF".

hex(S) -> binary:decode_hex(list_to_binary(S)).
