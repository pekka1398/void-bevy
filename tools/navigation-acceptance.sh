#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
binary="target/acceptance/void-app-navigation"
if [[ ! -x "$binary" || ! -f target/acceptance/navigation-SHA256SUMS ]]; then
    echo 'Missing navigation acceptance build; see docs/orbit-navigation.md.' >&2
    exit 1
fi
sha256sum --status -c target/acceptance/navigation-SHA256SUMS
if [[ $# -eq 0 ]]; then
    fixture="lab-log/navigation/departure-ready.json"
    if [[ ! -f "$fixture" ]]; then
        echo 'Missing staged orbital fixture; recreate it using docs/orbit-navigation.md.' >&2
        exit 1
    fi
    set -- --load "$fixture" --navigation-target selene
fi
exec "$binary" --body aurelia --view orbit "$@"
