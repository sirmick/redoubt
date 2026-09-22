#!/usr/bin/env bash
#
# ssh-key-ensure.sh -- create the sandbox's SSH identity once.
#
# The container does not mount the host's ~/.ssh. Instead the sandbox gets its own identity,
# generated on first start, under the persistent config directory (/config/ssh, which dev.sh
# bind-mounts from ../redoubt-config). It survives container restarts, so this is a one-time
# event; delete the key to regenerate it.
#
# dev.sh sets SSH_KEY_DIR=/config/ssh. Add the public key this prints to GitHub
# (Settings -> SSH and GPG keys) to push from inside the container.
set -uo pipefail

KEY_DIR="${SSH_KEY_DIR:-/config/ssh}"
KEY="${KEY_DIR}/id_ed25519"

# Already generated (this run or an earlier container): nothing to do.
if [ -s "$KEY" ]; then
    exit 0
fi

command -v ssh-keygen >/dev/null 2>&1 || {
    echo "warning: ssh-keygen not found; cannot create a sandbox SSH key" >&2
    exit 0
}

mkdir -p "$KEY_DIR" 2>/dev/null && chmod 700 "$KEY_DIR" 2>/dev/null
if ! ssh-keygen -q -t ed25519 -N '' -C 'redoubt-dev-sandbox' -f "$KEY" 2>/dev/null; then
    echo "warning: could not generate an SSH key at $KEY" >&2
    exit 0
fi
chmod 600 "$KEY" 2>/dev/null
chmod 644 "${KEY}.pub" 2>/dev/null

echo
echo "==> generated the sandbox SSH key (first run): ${KEY}"
echo "==> add this public key to GitHub (Settings -> SSH and GPG keys), then push:"
echo
cat "${KEY}.pub"
echo
exit 0