# beamlet

A small BEAM (Erlang/Elixir) interpreter in safe Rust, for the redoubt64 microkernel. Security,
auditability and simplicity come first. See [DESIGN.md](DESIGN.md).

    . tools/env.sh                  # the pinned OTP 28 / Elixir 1.20 toolchain
    cargo test                      # unit tests and hostile-input tests
    tools/difftest                  # differential tests against the real BEAM
    beamlet -pa DIR MODULE [FUNCTION]
