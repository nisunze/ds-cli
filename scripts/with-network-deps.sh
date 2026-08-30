#!/usr/bin/env bash
# Run one command with every native path dependency at Cargo's sibling paths.
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
expected_web_core_sha=$(tr -d '\r\n' <"$repo_root/pins/ds-client-core.rev")
if [[ ! "$expected_web_core_sha" =~ ^[0-9a-f]{40}$ ]]; then
    echo "with-network-deps: pins/ds-client-core.rev is not one exact Git SHA" >&2
    exit 66
fi
expected_command_kernel_sha=$(tr -d '\r\n' <"$repo_root/pins/ds-command-kernel.rev")
if [[ ! "$expected_command_kernel_sha" =~ ^[0-9a-f]{40}$ ]]; then
    echo "with-network-deps: pins/ds-command-kernel.rev is not one exact Git SHA" >&2
    exit 66
fi
common_git_dir=$(git -C "$repo_root" rev-parse --path-format=absolute --git-common-dir)
main_checkout=$(dirname "$common_git_dir")
network_checkout=$(dirname "$main_checkout")/ds-network
main_network_checkout=$network_checkout
web_checkout=$(dirname "$main_checkout")/ds-web
command_kernel_checkout=$(dirname "$main_checkout")/ds-command-kernel
required_network_link=$(dirname "$repo_root")/ds-network
required_web_link=$(dirname "$repo_root")/ds-web
required_command_kernel_link=$(dirname "$repo_root")/ds-command-kernel
if [[ -e "$required_network_link" || -L "$required_network_link" ]]; then
    network_checkout=$(realpath "$required_network_link")
fi
if [[ -e "$required_web_link" || -L "$required_web_link" ]]; then
    web_checkout=$(realpath "$required_web_link")
fi
if [[ -e "$required_command_kernel_link" || -L "$required_command_kernel_link" ]]; then
    command_kernel_checkout=$(realpath "$required_command_kernel_link")
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
if [[ "$(git -C "$network_checkout" rev-parse HEAD)" != "$(git -C "$main_network_checkout" rev-parse HEAD)" ]]; then
    echo "with-network-deps: ds-network must match the main checkout's exact source revision" >&2
    exit 66
fi
if [[ -n "$(git -C "$network_checkout" status --porcelain --untracked-files=normal -- Cargo.toml Cargo.lock crates)" ]]; then
    echo "with-network-deps: ds-network Cargo inputs differ from its pinned commit" >&2
    exit 66
fi
if [[ ! -f "$web_checkout/crates/ds-client-core/Cargo.toml" ]]; then
    echo "with-network-deps: expected $web_checkout/crates/ds-client-core/Cargo.toml" >&2
    exit 66
fi
if [[ "$(git -C "$web_checkout" remote get-url origin)" != *nisunze/ds-web.git ]]; then
    echo "with-network-deps: $web_checkout is not the nisunze/ds-web checkout" >&2
    exit 66
fi
if [[ "$(git -C "$web_checkout" rev-parse HEAD)" != "$expected_web_core_sha" ]]; then
    echo "with-network-deps: ds-web must be pinned to $expected_web_core_sha" >&2
    exit 66
fi
if [[ -n "$(git -C "$web_checkout" status --porcelain -- crates/ds-client-core)" ]]; then
    echo "with-network-deps: ds-web client core differs from its pinned commit" >&2
    exit 66
fi
if [[ ! -f "$command_kernel_checkout/Cargo.toml" ]]; then
    echo "with-network-deps: expected $command_kernel_checkout/Cargo.toml" >&2
    exit 66
fi
if [[ "$(git -C "$command_kernel_checkout" remote get-url origin)" != *nisunze/ds-command-kernel.git ]]; then
    echo "with-network-deps: $command_kernel_checkout is not the nisunze/ds-command-kernel checkout" >&2
    exit 66
fi
if [[ "$(git -C "$command_kernel_checkout" rev-parse HEAD)" != "$expected_command_kernel_sha" ]]; then
    echo "with-network-deps: ds-command-kernel must be pinned to $expected_command_kernel_sha" >&2
    exit 66
fi
if [[ -n "$(git -C "$command_kernel_checkout" status --porcelain --untracked-files=normal -- Cargo.toml Cargo.lock src)" ]]; then
    echo "with-network-deps: ds-command-kernel native inputs differ from its pinned commit" >&2
    exit 66
fi

created_network_link=0
created_web_link=0
created_command_kernel_link=0
cleanup() {
    if ((created_network_link)) && [[ -L "$required_network_link" ]] \
        && [[ "$(realpath "$required_network_link")" == "$(realpath "$network_checkout")" ]]; then
        unlink "$required_network_link"
    fi
    if ((created_web_link)) && [[ -L "$required_web_link" ]] \
        && [[ "$(realpath "$required_web_link")" == "$(realpath "$web_checkout")" ]]; then
        unlink "$required_web_link"
    fi
    if ((created_command_kernel_link)) && [[ -L "$required_command_kernel_link" ]] \
        && [[ "$(realpath "$required_command_kernel_link")" == "$(realpath "$command_kernel_checkout")" ]]; then
        unlink "$required_command_kernel_link"
    fi
}
trap cleanup EXIT HUP INT TERM

ensure_link() {
    local required=$1 source=$2 label=$3 flag=$4
    if [[ -e "$required" || -L "$required" ]]; then
        if [[ "$(realpath "$required")" != "$(realpath "$source")" ]]; then
            echo "with-network-deps: $required already exists and is not the main checkout's $label" >&2
            exit 73
        fi
    else
        ln -s "$source" "$required"
        printf -v "$flag" 1
    fi
}
ensure_link "$required_network_link" "$network_checkout" ds-network created_network_link
ensure_link "$required_web_link" "$web_checkout" ds-web created_web_link
ensure_link "$required_command_kernel_link" "$command_kernel_checkout" ds-command-kernel created_command_kernel_link

"$@"
