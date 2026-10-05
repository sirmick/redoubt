#!/usr/bin/env bash
set -euo pipefail

id -un
id -gn dev
getent passwd dev
getent group dev
command -v podman
command -v newuidmap
command -v newgidmap
mkdir -m 700 -p "$XDG_RUNTIME_DIR"
unshare -Ur true
sg dev -c 'podman --cgroup-manager=cgroupfs info --format "{{.Store.GraphDriverName}}"'
sg dev -c 'podman --cgroup-manager=cgroupfs --version'
