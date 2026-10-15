# B68: crypto's modular exponentiation holds its operands before it runs

- **Base:** main c8422f745. **Head:** wp-B68 08355c35d (one commit, amended after review round 1;
  worktree /home/mcloonan/redoubt/.worktrees/B68). Superseded head: ec81dd0ec.

## Review round 1 (steward-red-2: OK with notes), folded in 08355c35d

- P2-1: the modulus, the base and the exponent are each held to `MAX_MODULUS_BITS` absolutely
  (no longer base/exponent to the modulus's width); the base is reduced mod `m` before `modpow`,
  so every in-bound call answers as BEAM does: `mod_pow(1024, 1, 1000) = 24`,
  `mod_pow(2, 1024, 1000) = 216`, `(2^1024+5)^3 mod 1000007 = 751391`, `w^w mod 1000 = 375` for
  the 10,000-bit all-ones `w` (all computed with Python `pow`), and for DH a `G` of `P + 2` and an
  exponent of `a + P - 1` give the same public value; one bit over the bound in any operand is
  still refused under 10 ms. `dh_group` holds `G` to the bound, not to `P`'s width. The
  beamlet.md Limits bullet states the bound as it now is. **The "where it departs from OpenSSL"
  passage needs no new entry: no in-bound call departs from BEAM** (the only difference is the
  refusal over 10,000 bits, which the Limits bullet states).
- P3-1: `mod_pow`'s comment names the decode into a `BigUint` as the linear cost left and the VM's
  2^30-bit binary limit as what bounds it.
- Gates rerun on 08355c35d: crypto host tests 11/11 (rc 0); difftest cryptotests 6/6 (rc 0);
  cargo testbench docs PASS; nightly rustfmt + `git diff --check` clean. Prebuilt not rerun (the
  fold changes no interface; built once on ec81dd0ec).
- **Paths:** userland/otp/crypto/src/pk.rs; docs/userland/beamlet.md; docs/SUMMARY.md;
  docs/plan/m1-separation.md; docs/todo/beamlet-bignum-bounds.md (deleted).

## The bounds, and why

- `MAX_MODULUS_BITS = 10_000`: OpenSSL's `OPENSSL_DH_MAX_MODULUS_BITS`, the plan's figure. Every
  `modpow` in the crate (the three natives; the RSA paths are the `rsa` crate's, held already by
  `rsa_private_of`) runs through one `mod_pow(base, exp, m) -> Result<BigUint, Wide>`, which
  refuses `m`, then `base`, then `exp` over the limit, in bits (`BigUint::bits()`), before any
  arithmetic, then reduces the base mod `m` (review round 1; the first head held base and
  exponent to the modulus's width). The worst case allowed is a 10,000-bit exponent over a
  10,000-bit modulus.
- `dh_group(p, g)` (pure, tested) keeps the old checks (`P > 3`, `G ≠ 0`) and adds `P` and `G`
  each within the limit; `dh_params` maps a refusal to the existing
  `badarg(arg, "Bad DH parameters")`, so `compute_key` refuses a hostile `P` before it reads the
  peer's value or the private exponent.
- Error mapping (`too_wide`): `badarg` on the operand's own argument, messages "Base too large",
  "Exponent too large", "Modulus too large", in the crypto.erl `?nif_call` shape every other
  refusal here uses; never a panic (nothing allocates or divides before the check beyond
  `from_bytes_be`, which is linear).
- No in-bound call departs from BEAM (since round 1: a base wider than the modulus is reduced, as
  BEAM does). The only difference is the refusal over 10,000 bits, stated in the Limits bullet.

## Gates

| gate | command (from the worktree, env per the assignment) | result |
| --- | --- | --- |
| host tests per native | `scripts/q run -- cargo test --manifest-path userland/otp/Cargo.toml -p beamlet-crypto` | 11 passed, 0 failed (rc 0): `mod_exp_refuses_an_oversized_operand_before_it_runs`, `dh_generate_key_refuses_oversized_parameters_before_it_runs`, `dh_compute_key_refuses_an_oversized_private_key_before_it_runs`, each with an 8 MiB operand and a one-bit-over operand refused under 10 ms (debug build), and in-bound vectors: `pubkey.erl`'s `3^1000 mod 1000007 = 297623`, `2^10 mod 1000 = 24`, the widest (10,000-bit) modulus, `5^6 mod 23 = 8`, and a group-14 agreement whose shared secret was computed outside the crate (Python `pow`). |
| the negative, measured once | temporary ignored test, `--release`, then removed | unbounded `x.modpow(&x, &x)`: 2^14-bit operands 1.18 s, 2^16-bit (8 KiB) 72.5 s; 2^18-bit stopped after 10 min. Quoted in `refused_at_once`'s comment. |
| difftest | `scripts/q run --cores 8 -- tools/difftest cryptotests` (in userland/otp) | 6/6 passed, 0 failed (rc 0); log .tmp/b68-difftest.log |
| docs checker | `scripts/q run -- cargo testbench docs` | PASS 7.0 s (rc 0) |
| formatting | `rustfmt +nightly --config skip_children=true` on pk.rs; `git diff --check` | clean |
| clippy | `cargo clippy -p beamlet-crypto --tests` | 2 warnings, both pre-existing lines (pk.rs:621, :782), none in this change |
| build gate | `make -f scripts/jobs.mk prebuilt` | rc 0: both widths built (rv32: 284 cases, 0 failed, 195.7 s); log target/jobs/prebuilt.log in the worktree |
| ./test-shell beamlet stages | not run | the shell uses crypto only for `hash*` and `strong_rand_bytes` (userland/shell/lib/redoubt/util.ex:234,293; editor/files.ex:128); no DH or mod_pow |
| tests/size-budget.toml | no change | no beamlet crate has an entry |
| machine case | none | no crypto case exists |
| unsafe | none added | |

## Summaries checked

- docs/userland/beamlet.md, Limits inside one VM: the CPU bullet now states the bound (10,000 bits
  for the modulus and `P`, base and exponent to its width, RSA keys 8,192 bits with `D` and primes
  to `N`'s width; `badarg` before any arithmetic); the residual and todo link are gone. Status
  list: the three new host tests and `an_oversized_rsa_private_key_is_refused_before_it_is_built`
  (not listed anywhere before, and it backs the RSA sentence) added; tested (35) → (39).
- docs/userland/beamlet.md, What runs on it (crypto bullet): no change; it describes the
  implementation, not the limit, and its OpenSSL departures are about Ed25519.
- docs/plan/m1-separation.md Remaining work: the "follow-up packages" bullet listed only this
  page, so the bullet goes.
- docs/SUMMARY.md: the todo entry removed. docs/todo/README.md has no per-page lines: no change.
- README.md, GETTING-STARTED.md, docs/userland/README.md: no mention of crypto, mod_pow or the
  follow-up; no change. No M2 plan page mentions it.

## Risks / notes

- `BigUint::bits()` is used, not byte length: a 10,000-bit modulus whose binary carries leading
  zero bytes is still in bound, as OpenSSL counts.
- B50 edits beamlet.md's Life section; this change touches only the Limits section's CPU bullet
  and the status list: no line overlap expected.
