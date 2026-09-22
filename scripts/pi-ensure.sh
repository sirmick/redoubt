#!/usr/bin/env bash
#
# Ensure the dev environment's pi extensions are present in the active pi config.
#
# dev.sh bind-mounts ../redoubt-config at /config and points PI_CODING_AGENT_DIR at
# /config/pi, which shadows whatever the image installed at build time. This runs at
# container start and installs any missing extension into the active config there, so the
# extensions survive a respin along with the rest of the config. Idempotent and quiet: a
# no-op once installed. Needs network the first time.
set -uo pipefail

command -v pi >/dev/null 2>&1 || exit 0

for pkg in pi-subagents pi-deepseek-optimized; do
    if ! pi list 2>/dev/null | grep -q "$pkg"; then
        if ! pi install "npm:$pkg" >/dev/null 2>&1; then
            echo "warning: could not install pi extension '$pkg' (offline?)" >&2
        fi
    fi
done

exit 0