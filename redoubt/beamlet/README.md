# beamlet

A small BEAM (Erlang/Elixir) interpreter in safe Rust: the Elixir userland of Redoubt (it is not
part of the TCB). Security, auditability and simplicity come first. See [DESIGN.md](DESIGN.md), and
`planning/redoubt/` for the system it runs on (`BUILD-PLAN.md` WP-B1 is its platform).

Its own cargo workspace, excluded from the repository's: build it with `cargo` inside this
directory. Apache-2.0, as the rest of the repository (`LICENSES/`).

    . tools/env.sh                  # the pinned OTP 28 / Elixir 1.20 toolchain
    cargo test                      # unit tests and hostile-input tests
    tools/difftest                  # differential tests against the real BEAM
    beamlet -pa DIR MODULE [FUNCTION]
