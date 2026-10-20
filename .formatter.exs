# The Elixir outside userland/shell, which has a .formatter.exs of its own and is a subdirectory
# here, so `mix format` from this directory formats the whole tree. 110 columns, as the
# repository's rustfmt.toml sets for Rust. The generated modules (libs/wire/elixir/proto and
# client, libs/steward/elixir/gen) are their generators' output and are not inputs.
[
  inputs: [
    "libs/steward/elixir/*.ex",
    "libs/wire/elixir/{lib,test}/*.ex",
    "tools/doccheck/tests/fixtures/**/*.ex",
    "tools/testbench/src/*.exs",
    "userland/otp/tests/elixir/*.ex"
  ],
  subdirectories: ["userland/shell"],
  line_length: 110
]
