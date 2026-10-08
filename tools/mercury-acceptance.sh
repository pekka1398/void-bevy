#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
mode="${1:-orbit}"
if (($#)); then shift; fi
binary="$project_dir/target/acceptance/void-app-mercury"
if [[ ! -x "$binary" ]]; then
    echo "Missing Mercury acceptance binary: $binary" >&2
    echo "Build in this worktree: cargo build -p void-app -j 2; mkdir -p target/acceptance; cp target/debug/void-app target/acceptance/void-app-mercury" >&2
    exit 1
fi
cd "$project_dir"
case "$mode" in
    near|orbit|far) exec "$binary" --body cinder --view "$mode" "$@" ;;
    basin|rim|ejecta) exec "$binary" --cinder-site "$mode" "$@" ;;
    *) echo "Usage: $0 near|orbit|far|basin|rim|ejecta [main-game options]" >&2; exit 2 ;;
esac
