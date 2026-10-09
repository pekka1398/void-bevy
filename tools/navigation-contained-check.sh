#!/usr/bin/env bash
# Diagnostic/build runner, not the withdrawn user-facing game launcher.
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
limit=600
env_args=("--setenv=PATH=$PATH" "--setenv=CARGO_TARGET_DIR=$project_dir/../void-bevy/target")
if [[ "${1:-}" == --software-render ]]; then
    shift
    limit=120
    icd=/usr/share/vulkan/icd.d/lvp_icd.json
    [[ -f "$icd" ]] || { echo 'Missing explicit software Vulkan driver; refusing to run.' >&2; exit 1; }
    env_args+=("--setenv=VK_DRIVER_FILES=$icd" "--setenv=VK_ICD_FILENAMES=$icd"
        --setenv=WGPU_BACKEND=vulkan --setenv=WGPU_ADAPTER_NAME=llvmpipe --setenv=LP_NUM_THREADS=2)
fi
[[ "${1:-}" == -- ]] && shift
[[ $# -gt 0 ]] || { echo 'Usage: navigation-contained-check.sh [--software-render] -- command args...' >&2; exit 2; }
exec systemd-run --user --wait --pipe --collect --unit="void-navigation-check-$$" \
    -p MemoryHigh=2G -p MemoryMax=3G -p MemorySwapMax=0 -p CPUQuota=200% \
    -p TasksMax=96 -p Nice=10 -p IOWeight=10 -p RuntimeMaxSec="$limit" \
    -p TimeoutStopSec=3 -p KillMode=control-group -p OOMPolicy=kill \
    -p PrivateDevices=yes --working-directory="$project_dir" \
    "${env_args[@]}" \
    /usr/bin/python3 tools/navigation-check-exec.py "$@"
