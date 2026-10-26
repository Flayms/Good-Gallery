#!/usr/bin/env bash
# Runs one bench: bench.sh <name> <file> [extra args...]. Results in results/<name>.json.
set -uo pipefail
cd "$(dirname "$0")"
V="${V:-$TEMP/gg-spike/video}"
name=$1 file=$2; shift 2
timeout 120 ./target/release/spike-video.exe --maximized on --file "$V\\$file" --bench "$name" --seconds "${SECONDS_PER_RUN:-20}" "$@" > /dev/null 2>&1
node summarize.js "results/$name.json"
