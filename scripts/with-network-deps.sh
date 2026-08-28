#!/usr/bin/env bash
# Run one command with ds-network available at Cargo's declared relative path.
#
# A linked git worktree lives below the main ds-cli checkout, so Cargo's
# `../ds-network` path dependencies resolve beside the worktree rather than
# beside the main checkout. This wrapper obtains the exact sibling checkout
# from git's common directory, creates that one missing link only for the
# child command, and removes only the link it created.
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
required_link=$(dirname "$repo_root")/ds-network

for crate in ds-grid-model ds-grid-engine ds-grid-exchange ds-grid-tasks ds-io; do
    if [[ ! -f "$network_checkout/crates/$crate/Cargo.toml" ]]; then
        echo "with-network-deps: expected $network_checkout/crates/$crate/Cargo.toml" >&2
        exit 66
    fi
done

created_link=0
if [[ -e "$required_link" || -L "$required_link" ]]; then
    if [[ "$(realpath "$required_link")" != "$(realpath "$network_checkout")" ]]; then
        echo "with-network-deps: $required_link already exists and is not the main checkout's ds-network" >&2
        exit 73
    fi
else
    ln -s "$network_checkout" "$required_link"
    created_link=1
fi

cleanup() {
    if ((created_link)) && [[ -L "$required_link" ]] \
        && [[ "$(realpath "$required_link")" == "$(realpath "$network_checkout")" ]]; then
        unlink "$required_link"
    fi
}
trap cleanup EXIT HUP INT TERM

"$@"
