#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
binary="target/acceptance/void-app-navigation-performance"
fixture="target/acceptance/navigation-performance-v34.json"
if [[ ! -x "$binary" || ! -f "$fixture" || ! -f target/acceptance/navigation-performance-SHA256SUMS ]]; then
    echo 'Missing acceptance artifacts; see docs/navigation-performance.md.' >&2
    exit 1
fi
sha256sum --status -c target/acceptance/navigation-performance-SHA256SUMS
exec "$binary" --load "$fixture" --view orbit "$@"
