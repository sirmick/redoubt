# beamlet

A small BEAM (Erlang/Elixir) interpreter in safe Rust, for the redoubt64 microkernel. Security,
auditability and simplicity come first. It currently runs on the host; the Redoubt platform is
planned. See [its page](../../docs/userland/beamlet.md).

Differential/Elixir tests require separately installed OTP 28.5.0.6 and Elixir 1.20.4.
Neither this repository nor the Docker image installs them. `tools/env.sh` only adjusts PATH;
see [Getting started](../../GETTING-STARTED.md#7-beamlet-the-beam-vm) for the expected layout.

    . tools/env.sh                  # the pinned OTP 28 / Elixir 1.20 toolchain
    cargo test                      # unit tests and hostile-input tests
    tools/difftest                  # differential tests against the real BEAM
    beamlet -pa DIR MODULE [FUNCTION]
