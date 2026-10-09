#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
binary="target/acceptance/void-app-bodies"
if [[ ! -x "$binary" || ! -f target/acceptance/bodies-SHA256SUMS ]]; then
    echo 'Missing expanded bodies acceptance build; see docs/specs/expanded-bodies.md.' >&2
    exit 1
fi
sha256sum --status -c target/acceptance/bodies-SHA256SUMS
body="${1:-phobos}"
if (($#)); then shift; fi
exec "$binary" --body "$body" --view orbit "$@"
