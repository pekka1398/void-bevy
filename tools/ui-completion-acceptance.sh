#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
binary="target/acceptance/void-app-ui-completion"
if [[ ! -x "$binary" || ! -f target/acceptance/ui-completion-SHA256SUMS ]]; then
    echo 'Missing UI completion acceptance build; see docs/specs/ui-completion.md.' >&2
    exit 1
fi
sha256sum --status -c target/acceptance/ui-completion-SHA256SUMS
mode="${1:-flight}"
if (($#)); then shift; fi
case "$mode" in
    flight) options=() ;;
    rover) options=(--rover) ;;
    aircraft) options=(--aircraft) ;;
    orbit) options=(--body aurelia --view orbit) ;;
    venus) options=(--vesper-site plains --exposure 20) ;;
    *) echo "Usage: $0 flight|rover|aircraft|orbit|venus [game options]" >&2; exit 2 ;;
esac
exec "$binary" "${options[@]}" "$@"
