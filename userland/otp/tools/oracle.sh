# Sourced by the scripts that run an Elixir oracle on beamlet (libs/steward/elixir/run-traces,
# libs/wire/elixir/run-vectors), under `set -euo pipefail`: puts the pinned OTP and Elixir on the
# path (env.sh), builds beamlet with the all-Rust regex engine, the one that runs on Redoubt,
# unless BEAMLET names a prebuilt one, and defines
#
#   on_beamlet ROOT EBIN MODULE   runs MODULE's start/0 on beamlet with ROOT as the VM's /, and
#                                 on the code path EBIN, OTP's stdlib, kernel, erts, crypto and
#                                 compiler (the oracles' file modules and :crypto run unchanged)
#                                 and Elixir's own modules
tools="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$tools/env.sh"
BEAMLET="${BEAMLET:-$(RE_ENGINE=rust "$tools/build-beamlet")}"
otp_lib="$(dirname "$(dirname "$(command -v erl)")")/lib"
elixir_lib="$(dirname "$(dirname "$(command -v elixir)")")/lib"
beamlet_path=()
for app in stdlib kernel erts crypto compiler; do
    dirs=( "$otp_lib/$app"-*/ebin )
    beamlet_path+=( -pa "${dirs[0]}" )
done
beamlet_path+=( -pa "$elixir_lib/elixir/ebin" )

on_beamlet() { "$BEAMLET" --root "$1" -pa "$2" "${beamlet_path[@]}" "$3" start; }
