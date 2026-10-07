#!/usr/bin/env bash
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$project_root"
if [[ ! -x target/acceptance/void-app || ! -f target/acceptance/SHA256SUMS ]]; then
    echo 'Missing reviewed binary; see docs/next-features-acceptance.md for the explicit build procedure.' >&2
    exit 1
fi
(cd target/acceptance && sha256sum --status -c SHA256SUMS)
scenario=${1:-rover}
if (( $# > 0 )); then shift; fi
case "$scenario" in
    rover) options=(--rover) ;;
    aircraft) options=(--aircraft) ;;
    water) options=(--splashdown) ;;
    stars) options=(--stellar-neighborhood --stellar-fixture) ;;
    eva-space) options=(--rendezvous --craft crates/assembly/data/crewed-rocket.json) ;;
    *) echo 'Usage: scripts/run-next-features.sh rover|aircraft|water|stars|eva-space [game arguments]' >&2; exit 2 ;;
esac
exec target/acceptance/void-app "${options[@]}" "$@"
