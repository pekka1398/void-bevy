#!/usr/bin/env bash
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$project_root"
mkdir -p target
fixture_dir=$(mktemp -d "$project_root/target/interstellar-coast.XXXXXX")
fixture_path="$fixture_dir/cruise.world.json"
cargo run -p void-fleet-flight --example interstellar_coast_fixture -j 2 -- "$fixture_path"
printf 'Declared cruise starting state; not a completed transfer. Evidence: %s\n' "$fixture_dir"
exec cargo run -p void-app -j 2 -- --load "$fixture_path" --record "$fixture_dir/cruise.journal.json" "$@"
