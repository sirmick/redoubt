# beamlet

A small BEAM (Erlang/Elixir) interpreter in safe Rust, for the redoubt64 microkernel. Security,
auditability and simplicity come first. It runs on the host and on Redoubt, where it boots the
Elixir shell on the UART console and loads verified modules from the userland disk; its files are
9P files in its namespace, through the client library's hub. Native launching on Redoubt remains
planned. See [its page](../../docs/userland/beamlet.md).

Differential/Elixir tests require OTP 28.5.0.6 and Elixir 1.20.4. The dev container provides
both under `/opt/toolchains` and sets `BEAMLET_TOOLCHAINS` to it; on your own machine
`scripts/setup.sh --with-beam` builds them into the repository's untracked `toolchains/`.
`tools/env.sh` only adjusts PATH; see [Getting started](../../GETTING-STARTED.md#beamlet) for the
layout.

    . tools/env.sh                  # the pinned OTP 28 / Elixir 1.20 toolchain
    cargo test                      # unit tests and hostile-input tests
    tools/difftest                  # differential tests against the real BEAM, in parallel,
                                    # the BEAM's results cached per module
    beamlet -pa DIR MODULE [FUNCTION]
