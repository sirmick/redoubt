#!/usr/bin/env bash
# Provenance of vendor/ (vendor/README.md, "Provenance"): for each vendored crate, download the
# published .crate from static.crates.io, check its SHA-256 against the live crates.io index and
# against the checksum recorded in vendor/README.md, unpack it, and `diff -r` it against
# vendor/<name>. Needs the network, so the bench never runs it; the review of any change to
# vendor/ does. Exit 0 only if every crate matches on all three counts.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
readme="$root/vendor/README.md"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The crates.io sparse index path of a crate name (https://doc.rust-lang.org/cargo/reference/registry-index.html).
index_path() {
    local n="$1"
    case "${#n}" in
        1) echo "1/$n" ;;
        2) echo "2/$n" ;;
        3) echo "3/${n:0:1}/$n" ;;
        *) echo "${n:0:2}/${n:2:2}/$n" ;;
    esac
}

# The vendored table's header. Fail closed: a table that can't be found checks nothing.
header='^| Crate | Version | License (ours to use under)'
if ! grep -q "$header" "$readme"; then
    echo "provenance: no vendored table in $readme" >&2
    exit 1
fi

failed=0
checked=()
# Every row of the vendored table: | `name` | version | license | `sha256` |
while IFS='|' read -r _ name version _ sum _; do
    name="$(echo "$name" | tr -d ' `')"
    version="$(echo "$version" | tr -d ' ')"
    recorded="$(echo "$sum" | tr -d ' `')"
    [ -n "$name" ] || continue
    if [ ! -d "$root/vendor/$name" ]; then
        echo "$name $version: listed but vendor/$name is missing"
        failed=1
        continue
    fi
    checked+=("$name")

    crate="$work/$name-$version.crate"
    curl -sSfL -o "$crate" "https://static.crates.io/crates/$name/$name-$version.crate"
    downloaded="$(sha256sum "$crate" | cut -d' ' -f1)"
    indexed="$(curl -sSfL "https://index.crates.io/$(index_path "$name")" \
        | grep -F "\"vers\":\"$version\"" \
        | sed -n 's/.*"cksum":"\([0-9a-f]\{64\}\)".*/\1/p')"

    verdict=ok
    if [ "$downloaded" != "$indexed" ]; then verdict="download $downloaded != index $indexed"; fi
    if [ "$recorded" != "$indexed" ]; then verdict="README $recorded != index $indexed"; fi
    mkdir -p "$work/unpacked"
    tar -xzf "$crate" -C "$work/unpacked"
    if ! diff -r "$work/unpacked/$name-$version" "$root/vendor/$name" >"$work/$name.diff"; then
        verdict="vendor/$name differs from the published crate: $(wc -l <"$work/$name.diff") diff lines"
        cat "$work/$name.diff" >&2
    fi
    echo "$name $version: $verdict"
    [ "$verdict" = ok ] || failed=1
done < <(sed -n "/$header/,/^\$/p" "$readme" | tail -n +3)

# Every vendored directory must have been checked, except getrandom, which is patched and has its
# own section (vendor/README.md, "getrandom").
for dir in "$root"/vendor/*/; do
    name="$(basename "$dir")"
    [ "$name" = getrandom ] && continue
    if [[ " ${checked[*]} " != *" $name "* ]]; then
        echo "vendor/$name: not in the vendored table, so not checked"
        failed=1
    fi
done
if [ "${#checked[@]}" = 0 ]; then
    echo "provenance: the vendored table has no rows" >&2
    failed=1
fi

if [ "$failed" = 0 ]; then echo "provenance: every vendored crate is the published one"; fi
exit "$failed"
