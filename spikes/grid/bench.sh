#!/usr/bin/env bash
# One bench run; prints a one-line summary. Usage: ./bench.sh NAME [args...]
name=$1; shift
./target/release/spike-grid.exe --bench "$name" --seconds "${SECONDS_:-8}" "$@" 2>/dev/null | grep '^{' | python3 -c "
import sys,json
d=json.loads(sys.stdin.read()); i=d['interval_ms']; c=d['cpu_ms']; p=d['placeholder_rate']; w=d['worker_ms']
print(f\"{d['name']:22} int p50 {i['p50']:5.1f} p95 {i['p95']:5.1f} p99 {i['p99']:5.1f} | >20ms {d['over_20ms_per_1000']:4.0f}/k | cpu p95 {c['p95']:4.1f} | ph mean {p['mean']:.2f} p95 {p['p95']:.2f} | worker p50 {w['p50']:4.1f} | tex {d['texture_mb']} rss {d['rss_mb']} | 1st {d['first_screen_ms'] or 0:.0f}\")"
