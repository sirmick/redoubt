#!/usr/bin/env bash
#
# Ensure the dev environment's pi extensions are present in the active pi config.
#
# dev.sh bind-mounts the host's ~/.pi over /home/dev/.pi, which shadows whatever the image
# installed at build time. This runs at container start and installs any missing extension
# into whichever config is active (the mounted host one, or the image's when not mounted).
# Idempotent and quiet: a no-op once installed. Needs network the first time.
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