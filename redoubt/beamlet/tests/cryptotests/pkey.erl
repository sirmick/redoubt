-module(pkey).
-export([start/0]).
-include_lib("public_key/include/public_key.hrl").
%% public_key on top of crypto and the ASN.1 NIFs: PEM and DER, certificates (fields and
%% self-signatures), key generation, sign/verify, encrypt/decrypt. The certificates below
%% were made once with public_key:pkix_test_root_cert/2 on OTP 28.
start() ->
    Rsa = cert(<<"-----BEGIN CERTIFICATE-----\nMIIDrTCCApegAwIBAgIBATALBgkqhkiG9w0BAQswgYsxHzAdBgkqhkiG9w0BCQEW\nEHJvb3RAZXhhbXBsZS5vcmcxGjAYBgNVBAMTEWJlYW1sZXQgdGVzdCByb290MRIw\nEAYDVQQHEwlTdG9ja2hvbG0xCzAJBgNVBAYTAlNFMQ8wDQYDVQQKEwZlcmxhbmcx\nGjAYBgNVBAsTEWF1dG9tYXRlZCB0ZXN0aW5nMB4XDTI2MDkxNzEzMDAwMFoXDTI2\nMDkyNTEzMDAwMFowgYsxHzAdBgkqhkiG9w0BCQEWEHJvb3RAZXhhbXBsZS5vcmcx\nGjAYBgNVBAMTEWJlYW1sZXQgdGVzdCByb290MRIwEAYDVQQHEwlTdG9ja2hvbG0x\nCzAJBgNVBAYTAlNFMQ8wDQYDVQQKEwZlcmxhbmcxGjAYBgNVBAsTEWF1dG9tYXRl\nZCB0ZXN0aW5nMIIBIDALBgkqhkiG9w0BAQEDggEPADCCAQoCggEBALIHlyafH3i9\nInUAIgEEjUt0JjucPUeliAoLRJGFjY+mcbb2n2OUJtXYk2/S9PHZ6xl1/Ak+9Yv/\nQRnh+qGGV0BrR/kP6j6z9qqIWHWd8wlbzu+RJlMZn2djebT21Arz8etyKAh405t0\newpM1goJIlp27pYYzoh3PXWb456hqrBZTQMbWwFjP2hwsx1QA1yaODUtA8KrSHvU\nb14BKxkxVQtmQhBi47sVorm8ykVsNJbhQlE4kKjwEM2I9KFOzqumydSn5LnIC4Ql\nsZrfF2IM62t+me2mrRD8FqJv5/DoE2AO+in5R7fi7vIEGLWLT8G5QcL8utXetUYw\nFkpMOnpXT3ECAwEAAaMgMB4wCwYDVR0PBAQDAgGGMA8GA1UdEwEB/wQFMAMBAf8w\nCwYJKoZIhvcNAQELA4IBAQAzpQi+2yH6+fbD+68GWUh0DUgLgiZQNoSCHXc0jGxF\nGt/OxBKY2O5Fx2KvZZyBFa/ZNntW9NY02+OMdo+QNmPZodb86d4HV15ng1L9D4LV\nVKX2iptgvMM5D/zMbD/EvxelvNy9K+PP0Ri1RxkjFz51Nk/d/qDwAX8NFydf+O3a\nCsrwuHmb4zKTMNryu8g6qua2hLGg+7UDbWLdE5jTsVKFXyhBSQ+0ylcw+4S0pOfm\nmCgVr9uRDkjTPqB/POOPamJ8mWFJK8RsXDAGSZjq4nT6a18W5ghf+RSG87ibEbki\nlelpuCMuAtINnQLXaTjLhhKi904r2ehEgEdgf+EaUgps\n-----END CERTIFICATE-----\n\n">>),
    Ec = cert(<<"-----BEGIN CERTIFICATE-----\nMIICJjCCAcugAwIBAgIBAjAMBggqhkjOPQQDAgUAMIGJMR8wHQYJKoZIhvcNAQkB\nFhByb290QGV4YW1wbGUub3JnMRgwFgYDVQQDEw9iZWFtbGV0IGVjIHJvb3QxEjAQ\nBgNVBAcTCVN0b2NraG9sbTELMAkGA1UEBhMCU0UxDzANBgNVBAoTBmVybGFuZzEa\nMBgGA1UECxMRYXV0b21hdGVkIHRlc3RpbmcwHhcNMjYwOTE3MTMwMDAwWhcNMjYw\nOTI1MTMwMDAwWjCBiTEfMB0GCSqGSIb3DQEJARYQcm9vdEBleGFtcGxlLm9yZzEY\nMBYGA1UEAxMPYmVhbWxldCBlYyByb290MRIwEAYDVQQHEwlTdG9ja2hvbG0xCzAJ\nBgNVBAYTAlNFMQ8wDQYDVQQKEwZlcmxhbmcxGjAYBgNVBAsTEWF1dG9tYXRlZCB0\nZXN0aW5nMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEyR7nHgBRkkXWhVwvC7pK\nloB9KhJwT0WkSN1Ztf2gqGmC9uBh935uOblj1+iWHm5z3+3747jMT42W9NaBWSLI\n9KMgMB4wCwYDVR0PBAQDAgGGMA8GA1UdEwEB/wQFMAMBAf8wDAYIKoZIzj0EAwIF\nAANHADBEAiBSMq4HzFmPuvGXScwo6IwNKnLrP3sBOt6BhYZTXJQePAIgbF5Rcdes\n1RO/tq3O7SRGdunDvj5DAup40IEOTj0hPVU=\n-----END CERTIFICATE-----\n\n">>),
    Msg = <<"to be signed">>,
    {RPub, RPriv} = gen({rsa, 1024, 65537}),
    {EPub, EPriv} = gen({namedCurve, secp256r1}),
    {XPub, XPriv} = gen({namedCurve, ed25519}),
    Der = public_key:der_encode('RSAPublicKey', RPub),
    {Rsa, Ec,
     public_key:verify(Msg, sha256, public_key:sign(Msg, sha256, RPriv), RPub),
     public_key:verify(Msg, sha256, public_key:sign(Msg, sha256, EPriv), EPub),
     public_key:verify(Msg, none, public_key:sign(Msg, none, XPriv), XPub),
     public_key:verify(<<"forged">>, sha256, public_key:sign(Msg, sha256, EPriv), EPub),
     public_key:der_decode('RSAPublicKey', Der) =:= RPub,
     public_key:decrypt_private(public_key:encrypt_public(<<"secret">>, RPub), RPriv),
     length(public_key:pem_decode(public_key:pem_encode([{'RSAPublicKey', Der, not_encrypted}])))}.

gen(Params) ->
    Priv = public_key:generate_key(Params),
    {pub(Priv), Priv}.

pub(#'RSAPrivateKey'{modulus = N, publicExponent = E}) -> #'RSAPublicKey'{modulus = N, publicExponent = E};
pub(#'ECPrivateKey'{parameters = P, publicKey = Q}) -> {#'ECPoint'{point = Q}, P}.


cert(Pem) ->
    [{'Certificate', Der, not_encrypted}] = public_key:pem_decode(Pem),
    Otp = public_key:pkix_decode_cert(Der, otp),
    Tbs = Otp#'OTPCertificate'.tbsCertificate,
    Spki = Tbs#'OTPTBSCertificate'.subjectPublicKeyInfo,
    PubKey = Spki#'OTPSubjectPublicKeyInfo'.subjectPublicKey,
    Params = (Spki#'OTPSubjectPublicKeyInfo'.algorithm)#'PublicKeyAlgorithm'.parameters,
    Key = case PubKey of
              #'ECPoint'{} -> {PubKey, Params};
              _ -> PubKey
          end,
    {Tbs#'OTPTBSCertificate'.subject,
     Tbs#'OTPTBSCertificate'.serialNumber,
     Tbs#'OTPTBSCertificate'.validity,
     public_key:pkix_is_self_signed(Otp),
     public_key:pkix_verify(Der, Key),
     public_key:pkix_verify(corrupt(Der), Key),
     public_key:pkix_decode_cert(Der, plain) =:= public_key:der_decode('Certificate', Der),
     public_key:pkix_encode('OTPCertificate', Otp, otp) =:= Der}.

%% Flip a bit in the signature (the last byte of the certificate).
corrupt(Der) ->
    N = byte_size(Der) - 1,
    <<Head:N/binary, Last>> = Der,
    <<Head/binary, (Last bxor 1)>>.
