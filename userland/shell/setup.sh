# Sourced by ./shell and ./test-shell, from the repository's root, under `set -euo pipefail`:
# puts the pinned OTP and Elixir on the path, builds beamlet and the shell, and sets
#
#   otp, app     userland/otp and userland/shell, as absolute paths
#   beamlet      the beamlet binary ($BEAMLET if set; else built, with the all-Rust regex engine,
#                the one that runs on Redoubt, unless RE_ENGINE says otherwise)
#   path         the code path, as beamlet arguments: the system's modules first and the shell's
#                after them, as on Redoubt, where no directory shadows a system module
#   sandbox      the VM's / when none is named: userland/shell/_build/sandbox
#   native       userland/native, the cell protocol, whose vectors the tests read
#   fake         fake-redoubt: beamlet on Redoubt's platform, on the fake kernel, with the
#                console on this terminal and no file system or programs yet
#                (userland/otp/redoubt); it takes the code path and the module to run
#
# Linux is a development aid until beamlet runs on Redoubt: nothing is built for the host alone.

otp="$PWD/userland/otp"
app="$PWD/userland/shell"
native="$PWD/userland/native"
# A git worktree has no toolchains/ of its own (it is not tracked): use its main checkout's.
if [ -z "${BEAMLET_TOOLCHAINS:-}" ] && [ ! -d "$PWD/toolchains" ]; then
    main="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null || echo .)")"
    if [ -d "$main/toolchains" ]; then export BEAMLET_TOOLCHAINS="$main/toolchains"; fi
fi
. "$otp/tools/env.sh"
for tool in erl erlc elixir mix; do
    command -v "$tool" >/dev/null ||
        { echo "no pinned $tool on the path; see GETTING-STARTED.md, \"beamlet\"" >&2; exit 2; }
done
otp_lib="$(dirname "$(dirname "$(command -v erl)")")/lib"
elixir_lib="$(dirname "$(dirname "$(command -v elixir)")")/lib"
path=()
for lib in stdlib kernel compiler crypto public_key asn1 ssl ssh erts syntax_tools; do
    dirs=("$otp_lib"/"$lib"-*/ebin)
    [ -d "${dirs[0]}" ] || { echo "no $lib under $otp_lib" >&2; exit 2; }
    path+=(-pa "${dirs[0]}")
done
for lib in elixir logger eex ex_unit; do
    [ -d "$elixir_lib/$lib/ebin" ] || { echo "no $lib under $elixir_lib" >&2; exit 2; }
    path+=(-pa "$elixir_lib/$lib/ebin")
done

(cd "$app" && mix compile)
path+=(-pa "$app/_build/dev/lib/redoubt_shell/ebin")
beamlet="${BEAMLET:-$(RE_ENGINE="${RE_ENGINE:-rust}" "$otp/tools/build-beamlet")}"
(cd "$otp" && cargo build -q --release -p beamlet-redoubt --features fake)
fake="$otp/target/release/fake-redoubt"
sandbox="$app/_build/sandbox"
mkdir -p "$sandbox"
