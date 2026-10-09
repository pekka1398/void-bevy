#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
binary="target/acceptance/void-app-ui"
if [[ ! -x "$binary" || ! -f target/acceptance/ui-SHA256SUMS ]]; then
    echo 'Missing UI acceptance build; see docs/main-game-ui.md.' >&2
    exit 1
fi
sha256sum --status -c target/acceptance/ui-SHA256SUMS
mode="${1:-flight}"
if (($#)); then shift; fi
case "$mode" in
    flight) options=() ;;
    orbit) options=(--body aurelia --view orbit) ;;
    mars) options=(--ares-site plains) ;;
    venus) options=(--vesper-site plains --exposure 20) ;;
    rover) options=(--rover) ;;
    aircraft) options=(--aircraft) ;;
    water) options=(--splashdown) ;;
    stars) options=(--stellar-neighborhood --stellar-fixture) ;;
    *) echo "Usage: $0 flight|orbit|mars|venus|rover|aircraft|water|stars [game options]" >&2; exit 2 ;;
esac
exec "$binary" "${options[@]}" "$@"
