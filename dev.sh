#!/usr/bin/env bash
#
# dev.sh -- build the dev image (if needed) and drop into a shell with this workspace mounted.
#
# The whole point of this script is the mount list. Read it before changing it: every `-v`
# line is a path from your host that the container can see. Today that is:
#
#   - THIS directory           -> /work   (read-write: the project)
#   - the three CLIs' config/auth dirs    (so you do not re-login every run)
#
# NOT mounted: the rest of ~, ~/.ssh, ~/.config, other projects, and -- deliberately -- the
# Docker socket (mounting it would let anything inside the container become root on the host
# and defeat the sandbox entirely).
#
# Usage:
#   ./dev.sh                       # build if needed, then an interactive shell in /work
#   ./dev.sh cargo testbench       # build if needed, run one command, exit
#   ./dev.sh --rebuild             # rebuild the image first, then shell
#   ./dev.sh --no-agent-auth       # do not mount the CLI config/auth dirs
#   SANDBOX_NET=none ./dev.sh      # no network inside the container
set -euo pipefail

IMAGE=redoubt-dev
PROJECT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---- parse our own flags; everything else is the command -------------------
REBUILD=0
MOUNT_AGENT_AUTH=1
args=()
for a in "$@"; do
    case "$a" in
        --rebuild)        REBUILD=1 ;;
        --no-agent-auth)  MOUNT_AGENT_AUTH=0 ;;
        *)                args+=("$a") ;;
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

# ---- the mount list -------------------------------------------------------
# Every host path the container may see is spelled out here, one per line, with why.
# The `:z` suffix relabels the content for sharing with a container (the host runs SELinux in
# Enforcing mode, so a bind mount without a label suffix is unreadable inside).
mounts=(
    # The project. Read-write. This is the entire reason the sandbox exists.
    -v "${PROJECT}:/work:z"
)
if [ "$MOUNT_AGENT_AUTH" = 1 ]; then
    [ -d "${HOME}/.pi" ]          && mounts+=(-v "${HOME}/.pi:/home/dev/.pi:z")
    [ -d "${HOME}/.claude" ]      && mounts+=(-v "${HOME}/.claude:/home/dev/.claude:z")
    [ -f "${HOME}/.claude.json" ] && mounts+=(-v "${HOME}/.claude.json:/home/dev/.claude.json:z")
    [ -d "${HOME}/.codex" ]       && mounts+=(-v "${HOME}/.codex:/home/dev/.codex:z")
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
    -e CARGO_HOME=/work/.cargo \
    -e RUSTUP_HOME=/work/.rustup \
    -e TERM="${TERM:-xterm-256color}" \
    "${mounts[@]}" \
    -w /work \
    "$IMAGE" \
    bash -lc "${args[*]:-bash}"

# Caches under /work are disposable: rm -rf .cargo .rustup .cargo-target reclaims the space.