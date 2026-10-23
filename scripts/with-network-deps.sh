#!/usr/bin/env bash
# Run one command with the Network sibling at Cargo's path.
#
# Cargo resolves path dependencies beside this checkout. Components never pin
# each other (owner 23 Oct): the sibling is whatever ds-network checkout sits at
# that path (a release captures it at its pushed run tip). Create a missing link
# only for the child command and remove only that link.
set -euo pipefail

if (($# == 0)); then
    echo "usage: scripts/with-network-deps.sh <command> [args...]" >&2
    exit 64
fi

repo_root=$(git rev-parse --show-toplevel) || {
    echo "with-network-deps: run from a ds-cli checkout" >&2
    exit 64
}
common_git_dir=$(git -C "$repo_root" rev-parse --path-format=absolute --git-common-dir)
main_checkout=$(dirname "$common_git_dir")
network_checkout=$(dirname "$main_checkout")/ds-network
required_network_link=$(dirname "$repo_root")/ds-network
if [[ -e "$required_network_link" || -L "$required_network_link" ]]; then
    network_checkout=$(realpath "$required_network_link")
fi
for crate in ds-grid-model ds-grid-engine ds-grid-exchange ds-grid-tasks ds-io; do
    if [[ ! -f "$network_checkout/crates/$crate/Cargo.toml" ]]; then
        echo "with-network-deps: expected $network_checkout/crates/$crate/Cargo.toml" >&2
        exit 66
    fi
done
if [[ "$(git -C "$network_checkout" remote get-url origin)" != *nisunze/ds-network.git ]]; then
    echo "with-network-deps: $network_checkout is not the nisunze/ds-network checkout" >&2
    exit 66
fi
created_network_link=0
cleanup() {
    if ((created_network_link)) && [[ -L "$required_network_link" ]] \
        && [[ "$(realpath "$required_network_link")" == "$(realpath "$network_checkout")" ]]; then
        unlink "$required_network_link"
    fi

}
trap cleanup EXIT HUP INT TERM

ensure_link() {
    local required=$1 source=$2 label=$3 flag=$4
    if [[ -e "$required" || -L "$required" ]]; then
        if [[ "$(realpath "$required")" != "$(realpath "$source")" ]]; then
            echo "with-network-deps: $required already exists and is not the admitted $label" >&2
            exit 73
        fi
    else
        ln -s "$source" "$required"
        printf -v "$flag" 1
    fi
}
ensure_link "$required_network_link" "$network_checkout" ds-network created_network_link

"$@"
