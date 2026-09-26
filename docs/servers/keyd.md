# keyd

`keyd` holds every private key on the box. It signs; it never exports. Each badge on its endpoint
names one key and one purpose, and a purpose is the one message shape that badge may have signed,
built by `keyd` itself: an SSH key exchange for the host key, an audit record for the steward's
audit key. No caller chooses the bytes `keyd` signs, and no key it holds authenticates a person
to the box.

## Purpose

A key that leaks cannot be revoked; authority can. So private keys live in one small process,
separate from the steward and from `sshd`, which parse hostile input, and what leaves it is a
signature over a message shape `keyd` built, never the key and never a signature over bytes a
caller chose. A hijacked holder of a badge can then do what the badge's purpose is for, and
nothing more: it is not a signature oracle.

## Interface

### Keys and purposes

Status: built · partly tested: `keyd`'s host tests run in no bench case · tested: host:redoubt-keyd::arguments_become_keys_with_root_badges_in_order, host:redoubt-keyd::hostile_arguments_are_refused, host:redoubt-keyd::bad_key_arguments_stop_keyd_starting, host:redoubt-keyd::an_all_zero_seed_is_refused_rather_than_panicking, host:redoubt-keyd::two_keys_may_not_share_a_name_or_a_public_key, host:redoubt-keyd::there_is_a_bound_on_how_many_keys_there_are, host:redoubt-keyd::signatures_are_rfc_8032_ed25519, host:redoubt-keyd::hex_decoding_matches_the_obvious_decoder

`keyd` reads its keys from its startup block's arguments, one key per argument
([init](init.md#the-startup-block)), in the form `name,purpose,seed`:

- `name` follows the manifest's name rule; `,` is outside that rule, so no field can swallow the
  next.
- `purpose` is `ssh_host` (the box's SSH host key: `sign_ssh_exchange` only) or `audit` (the
  steward's audit key: `sign_record` only). There is no purpose that signs a caller's bytes as
  they came, and none for a key that authenticates a person.
- `seed` is exactly 64 lower-case hex digits, the 32-byte Ed25519 seed. An all-zero seed is
  refused (the signing crate would panic on it).

Every key is Ed25519 (RFC 8032), the one signature scheme on the box, so no message names an
algorithm. At most `MAX_KEYS` (16) keys; two keys may not share a name or a public key. Any bad
argument stops `keyd` starting: a key it cannot read is one it cannot sign with, and serving
without it would fail silently. There is no operation to add, replace or remove a key, so `keyd`
holds exactly what its startup block gave it for as long as it runs.

**Badges.** The key in position i of the arguments has root badge i + 1, so a restarted `keyd`
gives each root badge the same meaning without keeping anything. Badges at or above
`FIRST_MINTED_BADGE` are grants ([below](#granting-and-releasing)); any other badge names nothing.

### Messages

Status: built · partly tested: `keyd`'s host tests run in no bench case · tested: host:redoubt-keyd::a_signature_round_trips_over_the_real_ipc_path, host:redoubt-keyd::the_host_key_comes_from_keyd, host:redoubt-keyd::public_key_and_holds_answer_for_the_key_the_badge_names, host:redoubt-keyd::holds_answers_only_about_keys_that_are_here, host:redoubt-keyd::a_badge_signs_only_its_own_key_and_only_its_own_purpose, host:redoubt-keyd::the_preimage_is_the_rfc_4253_transcript, host:redoubt-keyd::the_hash_matches_an_independent_implementation, host:redoubt-keyd::parts_cannot_be_slid_into_each_other, host:redoubt-keyd::a_transcript_no_exchange_could_make_is_refused, host:redoubt-keyd::a_relayed_ssh_user_auth_blob_is_never_what_gets_signed, host:redoubt-keyd::fips_180_4_vectors, host:redoubt-keyd::chunking_never_changes_the_digest

`keyd` serves a typed protocol, not 9P: a file that answers `Tread` is the export it must not
have, and it has nothing to name. Every request first resolves the caller's badge to one key and
purpose and applies the label check to the key's labels; only then does any work happen.

- **`sign_ssh_exchange(v_c, v_s, i_c, i_s, q_c, q_s, k)`**, `ssh_host` only. `keyd` builds the
  SSH exchange hash (RFC 4253 section 8, `curve25519-sha256`, RFC 8731) itself from the transcript's
  parts, with the host key blob taken from its own key, never from the request, and signs that
  hash. The hash is also the session identifier. A transcript no key exchange could produce is
  `malformed`.
- **`sign_record(record)`**, `audit` only: a signature over `SHA-256("redoubt.audit.v1\0" ||
  u64_le(len) || record)`, which `keyd` computes itself. The steward's audit log uses it
  ([steward](steward.md#the-audit-log)).
- **`public_key`**: the badge's key's public half.
- **`holds(key)`**: whether any key `keyd` holds has this public key. `sshd` asks it before
  accepting a login key; `init` asks it before trusting the manifest
  ([R35 (key separation)](init.md#r35-key-separation)). It answers about every key, not only the
  badge's: public keys are published, and the asker already holds the one it asks about.
- **`grant`** and **`release(id)`**: below.

`keyd`'s SHA-256 (`servers/keyd/src/sha256.rs`) is its own, one function with no dependencies,
because the maintained crate brings six more into the process holding every private key.

The table: [libs/wire/tables/keyd.md](../../libs/wire/tables/keyd.md).

{{#include ../../libs/wire/tables/keyd.md:tables}}

### Granting and releasing

Status: built · partly tested: `keyd`'s host tests run in no bench case · tested: host:redoubt-keyd::a_launcher_grants_a_fresh_capability_and_releases_it, host:redoubt-keyd::a_granted_capability_cannot_grant_again, host:redoubt-keyd::a_granted_capability_names_the_same_key_and_dies_with_release, host:redoubt-keyd::release_zero_frees_everything_this_caller_granted, host:redoubt-keyd::a_grant_whose_reply_never_arrives_is_undone, host:redoubt-keyd::a_grant_that_fails_gives_its_admission_back, host:redoubt-keyd::serving_grant_rolls_back_discard_missing_capability_and_error, host:redoubt-keyd::a_stale_grant_does_not_name_a_new_key_after_a_restart

`keyd`'s grants follow the typed pattern ([wire](wire.md#granting-and-releasing)), over the serving
library's minted table ([minted connections](serving.md#minted-connections)):

- **`grant`** mints a fresh capability for the caller's own key and purpose, stamped like the
  handle the request came through, and replies with it and a random id. A launcher asks for one
  per child rather than passing its own on. Nothing granted is wider than the badge it came
  through.
- **Only a root badge may grant.** A granted capability cannot grant again, so grants never chain.
  An account-0 caller is admitted per badge, and a chain would let one daemon open a fresh bucket
  per link until nobody, the steward included, could grant at all
  ([serving](serving.md#residual-risks)).
- **`release(id)`** frees the grant and everything granted under it, for the caller that received
  `id` only; anyone else gets the same `not_permitted` as an id that does not exist.
  **`release(0)`** frees everything this caller granted: no grant has id 0, and a restarted holder
  that lost its ids needs it.
- A grant whose reply was discarded, or arrived without the capability, is undone
  ([replies and rollback](serving.md#replies-and-rollback)).

### Bounds and errors

Status: built · partly tested: `keyd`'s host tests run in no bench case · tested: host:redoubt-keyd::one_requests_work_is_bounded, host:redoubt-keyd::a_flood_takes_only_the_flooders_share, host:redoubt-keyd::a_hostile_client_does_not_hurt_keyd_or_other_clients, host:redoubt-keyd::the_limits_are_sized_as_containment_says, host:redoubt-keyd::malformed_requests_are_refused_and_their_handles_closed, host:redoubt-keyd::a_one_way_message_is_dropped_and_its_handles_closed, host:redoubt-keyd::random_requests_never_panic, host:redoubt-keyd::a_labelled_caller_reads_but_does_not_sign

- **One request's work is bounded:** each transcript part at most `MAX_PART` (16 KiB), a record at
  most `MAX_RECORD` (8 KiB); over it is `too_many`.
- **Admission** counts the one thing a client can make `keyd` hold, its grants: at most 8 live
  grants per (account, label set), across at most 16 of those at once (`LIMITS`), sized to fit
  `keyd`'s 256 KiB budget ([R26 (admission fairness)](serving.md#r26-admission-fairness)). The
  bucket count is compiled in, a departure from the rule that every shared server takes
  `buckets=N` from the manifest ([init](init.md#the-boot-manifest)).
  `keyd` parks no calls and keeps no other per-caller state, so a flood of signing requests grows
  it by nothing and is bounded by the kernel's fair waiting.
- **The label check.** `keyd`'s keys carry no labels, so anyone may read a public key or ask
  `holds`, and only an unlabelled caller may sign, grant or release: signing puts the caller's data
  into something the key vouches for, which is a write
  ([R25 (the label check)](serving.md#r25-the-label-check)).
- **Errors:** `not_permitted` for a badge that names no key, names a key whose purpose does not
  allow the operation, fails the label check, or is a grant asking to grant, and it says no more
  than that; `too_many` for a request over a bound or a client at its cap; `failed` for a mint the
  kernel refused; `malformed` for a request that does not decode. A one-way message is dropped and
  its handles closed.

### Seeds from the manifest

Status: planned · M1 (separation and containment)

The seeds are written in `keyd`'s `servers` entry in the boot manifest, which `init` passes
through unchanged as `keyd`'s arguments ([init](init.md#the-boot-manifest)). The manifest is
never public, so no session can read them. The manifest hands `sshd` the `ssh_host` key's root
badge and the steward the `audit` key's; the steward never holds the host key's badge.

**Open:** none.

### Running under `init`

Status: planned · M1 (separation and containment)

`init` starts `keyd` through the loader stub before the steward and `sshd`, then asks it `holds`
for every login and approval key and the bundle key ([init](init.md#the-key-separation-check)).
`keyd` answers calls on its endpoint until the endpoint is destroyed. Restarted, it reads the same
arguments and gives each root badge the same key, and every earlier grant names nothing.

**Open:** none.

### Sealed keys, labelled keys and keys in leases

Status: planned · M5 (persist, install, share)

- **Sealed keys generated at first boot.** `keyd` generates its keys on the box at first boot and
  seals them to the machine, so no seed is in a manifest or the bundle.
- **Labelled keys.** A key may carry labels, so a vault session can sign with a key of its label.
  `holds` then applies the label check per key, not only the badge's.
- **Keys in leases.** A lease may carry a principal's key only if its approval named the key, and
  then only with the one message shape it may sign, never arbitrary bytes, or a hijacked agent
  would be a signature oracle that lets its peer log in as its sponsor elsewhere
  ([steward](steward.md#leases)).
- **A `pkg` purpose.** A key with it signs only a package: the full preimage
  `"redoubt.pkg.v1\0" || u64_le(len) || archive` that `keyd` builds itself from the archive, in the
  loader's form, with an approval per signature ([packages](pkg.md#what-is-signed)). It is the one
  purpose whose signed message is not a 32-byte digest. It adds one shape to R44 and no more: a
  `pkg` signature covers only a preimage `keyd` built under the package domain, which is never an
  SSH exchange, an audit record or a bundle, since domains are prefix-free.

**Open:** where sealed keys are kept and what they are sealed with; the message shapes a
principal's key may sign; who enforces the approval per `pkg` signature, since `keyd` makes no
calls (the recommendation: the steward mints a single-use grant after each approval, and `keyd`
signs once per grant).

## Authority

Status: built · partly tested: `keyd`'s host tests run in no bench case · tested: host:redoubt-keyd::nothing_in_the_protocol_returns_a_key, host:redoubt-keyd::no_request_can_put_a_key_into_keyd, host:redoubt-keyd::a_badge_signs_only_its_own_key_and_only_its_own_purpose

- `keyd` holds its keys, its own endpoint and the handles it minted as grants. It makes no calls
  to any other server.
- A badge's authority is its key's purpose and nothing else: an `ssh_host` badge speaks as the box
  in a key exchange, with any peer, for as long as it is held, which is what a host-key capability
  is for.
- No operation returns a private key or any function of one but a signature, and none puts a key
  into `keyd`.
- The crate forbids `unsafe`.

## Security properties

### R43 (no export)

Status: built · partly tested: `keyd`'s host tests run in no bench case · tested: host:redoubt-keyd::nothing_in_the_protocol_returns_a_key, host:redoubt-keyd::no_request_can_put_a_key_into_keyd, host:redoubt-keyd::random_requests_never_panic

No message of `keyd`'s protocol returns a private key or a function of one other than a
signature, and none adds, replaces or removes a key. A caller cannot ask for what the protocol
cannot say, so the only way a key leaves `keyd` is a bug in `keyd` itself.

### R44 (one key, one purpose, keyd's own digest)

Status: built · partly tested: `keyd`'s host tests run in no bench case · tested: host:redoubt-keyd::a_badge_signs_only_its_own_key_and_only_its_own_purpose, host:redoubt-keyd::a_relayed_ssh_user_auth_blob_is_never_what_gets_signed, host:redoubt-keyd::parts_cannot_be_slid_into_each_other, host:redoubt-keyd::the_hash_matches_an_independent_implementation, host:redoubt-keyd::a_granted_capability_names_the_same_key_and_dies_with_release

A badge names one key and one purpose. Under the purposes `keyd` holds, `ssh_host` and `audit`,
every signature is over exactly 32 bytes, a digest `keyd` computed itself: an SSH exchange hash
over a transcript naming `keyd`'s own public key, or the SHA-256 of a fixed domain string, a length
and a record. So a holder of a badge gets signatures only in its purpose's shape, and no container
that covers longer messages (a boot bundle's archive, an SSH user-authentication request, which is
at least 36 bytes before its user name) can be what a `keyd` signature covers.

### R45 (constant-time signing)

Status: built · partly tested: the timing tests (`signing_takes_the_same_time_whatever_the_key`, `signing_takes_the_same_time_whatever_the_nonce` in `servers/keyd/tests/timing.rs`) are ignored in an ordinary test run and need an optimised build (`cargo test --release -p redoubt-keyd --test timing`), which no bench case runs

Signing takes the same time whatever the key: no branch and no memory index depends on secret
material, in the seed's decoding, in SHA-256, or in the Ed25519 crate `keyd` links (read at
version 2.4.2; `keyd` never verifies, so the crate's variable-time verification code is never
reached). The timing tests measure it fixed-against-random, with a deliberately leaking
signer as the control the same statistic must flag in the same run. They resolve a few per cent
of one signature; the claim itself rests on reading the code.

## Failure and restart

Status: built · partly tested: `keyd`'s host tests run in no bench case · tested: host:redoubt-keyd::bad_key_arguments_stop_keyd_starting, host:redoubt-keyd::a_stale_grant_does_not_name_a_new_key_after_a_restart, host:redoubt-keyd::release_zero_frees_everything_this_caller_granted

- **A bad key argument** stops `keyd` before it serves anything, loudly.
- **`keyd` restarts:** root badges mean the same keys again; every grant from before the restart
  names nothing (`not_permitted`), since the minted table starts empty with a new first badge
  ([R27 (badge allocation)](serving.md#r27-badge-allocation)).
- **A holder restarts** and lost its grant ids: it frees them with `release(0)`.

## Residual risks

- **The seeds live in `init`'s memory and the bundle.** They arrive in the read-only startup page,
  so they exist in `init`'s memory and in the signed, unencrypted bundle, and `keyd` cannot erase
  its copy. Whoever can read the bundle image holds the box's private keys. Sealed keys are
  planned (above).
- **`keyd` sees the SSH shared secret.** `sign_ssh_exchange` carries the key exchange's shared
  secret `k`, because the exchange hash covers it. A compromised `keyd` can derive any session's
  keys and read its traffic, not only speak as the box; `keyd` is written small enough to read.
- **`holds` answers about every key.** It reveals nothing a caller could not learn by connecting
  while keys are unlabelled; with labelled keys it needs a per-key check.
- **An `ssh_host` badge speaks as the box.** Its holder can complete a key exchange as the box with
  any peer, for as long as it holds the badge.
- **Its bucket count is compiled in,** so the manifest cannot size it. Follow-up:
  [todo](../todo/server-bucket-counts.md).
- **`keyd` does not boot in the bench.** Its behaviour is attacked by host tests against the
  runtime's fake kernel; `keyd-build` only builds it for both widths, and no bench case runs its
  host tests. Follow-up: [todo](../todo/host-tests-in-bench.md).

## Why

- **Separate from the steward.** A leaked key cannot be revoked; a steward bug that leaked
  authority can be. Keeping keys out of the process with the most hostile input keeps the
  unrevocable thing furthest from the attacker.
- **Purposes as message shapes.** A badge that signed any bytes would be a signature oracle: a
  hijacked holder could relay someone's SSH user-authentication request and log in as the box's
  owner elsewhere. A digest `keyd` builds cannot be one.
- **No enrolment.** An operation that adds a key is a way to make `keyd` hold a key that
  authenticates a person; with none, keys come only from the signed manifest.
- **Its own SHA-256.** About ninety lines read once are cheaper to trust than six crates inside the
  process that holds every private key.
- **Root badges by position.** A restarted `keyd` gives each root badge the same key without
  storing anything, and grants cannot survive it.
