#!/usr/bin/env bash
# Contract: ../ds-web/docs/deployment-contract.md#repository-deployment-entrypoints
# Release authority stays with the existing shared owner.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$ROOT/../ds-web/deploy-component.sh" kernel windows "$@"
