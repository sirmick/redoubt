#!/usr/bin/env bash
#
# dev.sh -- build the dev image (if needed) and run a shell, or one command, in it with this
# checkout mounted.
#
# The image is Debian with what scripts/setup.sh installs (see the Dockerfile). The container
# sees this checkout, read-write, and nothing else from the host: not your home, your keys, nor
# the Docker socket (which would let the container become root on the host). The checkout is
# mounted twice, at /work and at its own host path, so that a git worktree under it, whose .git
# file names the checkout's host path, works inside too.
#
# Usage:
#   ./dev.sh                       # build if needed, then a shell in /work
#   ./dev.sh cargo testbench       # build if needed, run one command, exit
#   ./dev.sh --rebuild [command]   # rebuild the image first
#   SANDBOX_NET=none ./dev.sh      # no network inside the container
#
# To throw the image away (the checkout is not touched):
#   docker rmi redoubt-dev && docker builder prune   # optional: reclaim the build cache too
#   ./dev.sh --rebuild
set -euo pipefail

IMAGE=redoubt-dev
IMAGE_REV=5
PROJECT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# The image contains a real passwd entry for the host identity. OpenSSH refuses to run for a
# numeric uid that NSS cannot resolve, so the uid/gid are part of the image's cache identity.
UID_N="$(id -u)"
GID_N="$(id -g)"

REBUILD=0
if [ "${1:-}" = --rebuild ]; then
    REBUILD=1
    shift
fi

# ---- the image ------------------------------------------------------------
label() { docker image inspect --format "{{ index .Config.Labels \"org.redoubt.dev.$1\" }}" "$IMAGE" 2>/dev/null || true; }
if [ "$REBUILD" = 1 ] || [ "$(label revision)" != "$IMAGE_REV" ] \
    || [ "$(label uid)" != "$UID_N" ] || [ "$(label gid)" != "$GID_N" ]; then
    echo "==> building $IMAGE (Debian, Rust, QEMU, and OTP built from source: a while the first time)"
    docker build \
        --build-arg "IMAGE_REV=$IMAGE_REV" \
        --build-arg "USER_UID=$UID_N" \
        --build-arg "USER_GID=$GID_N" \
        -t "$IMAGE" "$PROJECT"
fi

# ---- the project-local toolchain cache ------------------------------------
# The run below points RUSTUP_HOME/CARGO_HOME at /work/.rustup and /work/.cargo so the toolchain
# and crate cache live inside the checkout (survive restarts, delete with it). Seed them from
# the image the first time: the image already has the toolchain installed under /opt.
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
# A TTY only when there is one; `docker run -it` fails outright without a terminal.
tty_args=()
if [ -t 0 ] && [ -t 1 ]; then
    tty_args=(-it)
fi

# The `:z` suffix relabels the mount for a container, for hosts that run SELinux enforcing.
# CARGO_TARGET_DIR is deliberately NOT set: the bench looks for build artifacts at the fixed
# path <workspace>/target/<triple>/<profile>/, so build output lives in each crate tree's own
# target/ (git-ignored, disposable).
docker run --rm "${tty_args[@]}" \
    --hostname redoubt-dev \
    --user "${UID_N}:${GID_N}" \
    --network "${SANDBOX_NET:-bridge}" \
    -e HOME=/home/dev \
    -e CARGO_HOME=/work/.cargo \
    -e RUSTUP_HOME=/work/.rustup \
    -e TERM="${TERM:-xterm-256color}" \
    -v "${PROJECT}:/work:z" \
    -v "${PROJECT}:${PROJECT}:z" \
    -w /work \
    "$IMAGE" \
    "$@"

# Caches under /work are disposable: rm -rf .cargo .rustup reclaims the space.
