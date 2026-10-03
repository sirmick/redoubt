# Source this to put the pinned Erlang/Elixir toolchain on PATH. Versions: see README.md.
# The same versions are pinned in the Dockerfile's ARGs and in tests/*elixir-oracles*.toml (the
# bench's toolchain check): change all of them together.
TOOLCHAINS="${BEAMLET_TOOLCHAINS:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/toolchains}"
export PATH="$TOOLCHAINS/otp-28.5.0.6/bin:$TOOLCHAINS/elixir-1.20.4/bin:$PATH"
