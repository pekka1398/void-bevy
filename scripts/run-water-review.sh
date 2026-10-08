#!/usr/bin/env bash
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
speed=${1:-2}
tilt=${2:-0}
entry=${3:-0}
if (( $# >= 3 )); then shift 3; else set --; fi
exec "$project_root/scripts/run-next-features.sh" water \
    --water-speed "$speed" --water-tilt "$tilt" --water-entry-angle "$entry" "$@"
