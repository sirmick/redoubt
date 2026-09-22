#!/usr/bin/env bash
#
# dev.sh -- build the dev image (if needed) and drop into a shell with this workspace mounted.
#
# The whole point of this script is the mount list. Read it before changing it: every `-v`
# line is a path from your host that the container can see. Today that is:
#
#   - THIS directory           -> /work     (read-write: the project)
#   - ./redoubt-config         -> /config   (read-write: what survives a respin)
#
# Only those two. The host's ~/.pi, ~/.claude, ~/.codex, ~/.ssh and ~/.config are NOT mounted;
# nor is the Docker socket (mounting that would let anything inside the container become root
# on the host and defeat the sandbox entirely).
#
# redoubt-config/ is created next to this script if missing and holds everything that must
# outlive a container:
#   pi/          pi's config directory (PI_CODING_AGENT_DIR); copy your ~/.pi/agent contents here
#   ssh/         this sandbox's own SSH identity and known_hosts, generated on first start
#   gitconfig    git identity (GIT_CONFIG_GLOBAL), since /home/dev does not survive --rm
#
# To push from inside, add the public key printed on first start to GitHub (Settings -> SSH
# and GPG keys). Delete the key to regenerate it. The host's own SSH keys never enter the
# container.
#
# Usage:
#   ./dev.sh                       # build if needed, then an interactive shell in /work
#   ./dev.sh cargo testbench       # build if needed, run one command, exit
#   ./dev.sh --rebuild             # rebuild the image first, then shell
#   SANDBOX_NET=none ./dev.sh      # no network inside the container
#
# To throw the image away and rebuild from scratch (the source and redoubt-config are not
# touched):
#   docker rmi redoubt-dev && docker builder prune   # optional: reclaim the build cache too
#   ./dev.sh --rebuild
set -euo pipefail

IMAGE=redoubt-dev
PROJECT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---- parse our own flags; everything else is the command -------------------
REBUILD=0
args=()
for a in "$@"; do
    case "$a" in
        --rebuild)  REBUILD=1 ;;
        *)          args+=("$a") ;;
    esac
done

# ---- the image ------------------------------------------------------------
if [ "$REBUILD" = 1 ] || ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "==> building $IMAGE (this pulls Debian + Node + Rust; a few minutes the first time)"
    docker build -t "$IMAGE" "$PROJECT"
fi

# ---- host user ------------------------------------------------------------
UID_N="$(id -u)"
GID_N="$(id -g)"

# ---- the persistent config directory --------------------------------------
# A sibling of the project, so it survives a `docker rmi` and is never committed. Created here
# rather than hand-made, so a fresh clone can boot the environment with one command.
CONFIG="${REDOUBT_CONFIG:-${PROJECT}/../redoubt-config}"
mkdir -p "${CONFIG}/pi" "${CONFIG}/ssh"

# ---- the mount list -------------------------------------------------------
# Every host path the container may see is spelled out here, one per line, with why. This list
# is the whole sandbox boundary. The `:z` suffix relabels the content for sharing with a
# container (the host runs SELinux in Enforcing mode, so a bind mount without a label suffix is
# unreadable inside).
mounts=(
    # The project. Read-write. This is the entire reason the sandbox exists.
    -v "${PROJECT}:/work:z"
    # What survives a respin: pi's config and this sandbox's SSH key.
    -v "${CONFIG}:/config:z"
)

# Inside the container: pi's config directory and the sandbox's own SSH identity.
PI_CONFIG_DIR="/config/pi"
SSH_DIR="${CONFIG}/ssh"
GIT_SSH_KEY="${SSH_DIR}/id_ed25519"
GIT_KNOWN_HOSTS="${SSH_DIR}/known_hosts"
GIT_CONFIG_FILE="${CONFIG}/gitconfig"

# ---- the persistent SSH + git config --------------------------------------
# The sandbox has its own identity (never the host's) and refuses unknown host keys, so
# seed GitHub's host keys once. Without this the first push prompts, and any non-interactive
# push fails outright.
if [ ! -s "${GIT_KNOWN_HOSTS}" ]; then
    echo "==> seeding ${GIT_KNOWN_HOSTS} with GitHub host keys"
    ssh-keyscan -t rsa,ecdsa,ed25519 github.com >"${GIT_KNOWN_HOSTS}" 2>/dev/null ||
        echo "warning: ssh-keyscan failed; pushes to github.com may prompt" >&2
    chmod 644 "${GIT_KNOWN_HOSTS}" 2>/dev/null || true
fi

# git identity lives in /config because /home/dev is not mounted and --rm discards it.
if [ ! -s "${GIT_CONFIG_FILE}" ]; then
    echo "==> creating ${GIT_CONFIG_FILE} (git identity)"
    cat >"${GIT_CONFIG_FILE}" <<'GITCONFIG'
[user]
    name = mick
    email = sirmick@gmail.com
GITCONFIG
fi

# ---- bootstrap the project-local toolchain cache --------------------------
# The run below points RUSTUP_HOME/CARGO_HOME at /work/.rustup and /work/.cargo so the toolchain
# and crate cache live inside the project (survive restarts, delete with the project). Seed them
# from the image the first time: the image already has the toolchain installed under /opt.
if [ ! -e "${PROJECT}/.rustup/settings.toml" ] || [ ! -e "${PROJECT}/.cargo/bin/cargo" ]; then
    echo "==> seeding /work/.cargo and /work/.rustup from the image (first run)"
    docker run --rm \
        --user 0:0 \
        -v "${PROJECT}:/work:z" \
        "$IMAGE" \
        bash -c 'mkdir -p /work/.rustup /work/.cargo \
                 && cp -a /opt/rustup/. /work/.rustup/ \
                 && cp -a /opt/cargo/.  /work/.cargo/ \
                 && chown -R "$1:$2" /work/.rustup /work/.cargo' _ "${UID_N}" "${GID_N}"
fi

# ---- run ------------------------------------------------------------------
# Only allocate a TTY when we actually have one; otherwise `docker run -it` fails outright
# when called from a script or another tool without a terminal.
tty_args=()
if [ -t 0 ] && [ -t 1 ]; then
    tty_args=(-it)
fi

# --user: files created in /work stay owned by you, not root.
# -e CARGO_HOME/RUSTUP_HOME: keep the toolchain and the crate cache inside the project.
# NOTE: CARGO_TARGET_DIR is deliberately NOT set. The Redoubt test bench looks for build
# artifacts at the fixed path <workspace>/target/<triple>/<profile>/ and does not honour
# CARGO_TARGET_DIR, so redirecting it would make every boot case fail. Build output therefore
# lives in each crate tree's own target/ (git-ignored, disposable).
# SANDBOX_NET (default "bridge"): set to "none" to cut the container off the network.
docker run --rm "${tty_args[@]}" \
    --hostname redoubt-dev \
    --user "${UID_N}:${GID_N}" \
    --network "${SANDBOX_NET:-bridge}" \
    -e HOME=/home/dev \
    -e PI_CODING_AGENT_DIR="${PI_CONFIG_DIR}" \
    -e CARGO_HOME=/work/.cargo \
    -e RUSTUP_HOME=/work/.rustup \
    -e GIT_SSH_COMMAND="ssh -i /config/ssh/id_ed25519 -o IdentitiesOnly=yes -o UserKnownHostsFile=/config/ssh/known_hosts -o StrictHostKeyChecking=yes" \
    -e GIT_CONFIG_GLOBAL="/config/gitconfig" \
    -e TERM="${TERM:-xterm-256color}" \
    "${mounts[@]}" \
    -w /work \
    "$IMAGE" \
    # Create the sandbox SSH key on first start (idempotent, prints the public key), install
    # any missing pi extension into the mounted config dir, then run the command.
    bash -lc "SSH_KEY_DIR=/config/ssh /work/scripts/ssh-key-ensure.sh; /work/scripts/pi-ensure.sh; ${args[*]:-bash}"

# Caches under /work are disposable: rm -rf .cargo .rustup .cargo-target reclaims the space.
