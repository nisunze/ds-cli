#!/usr/bin/env bash
# Contract: ../ds-architecture/architecture/module-production.md#deploying-a-component
# This repository builds the component; release coordination and packaging are ds-architecture's.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$ROOT/../ds-architecture/scripts/release/deploy-component.sh" kernel linux "$@"
