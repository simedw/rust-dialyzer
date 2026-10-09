#!/bin/bash
# Full exact-parity suite for one core_cache_analyze binary.
#   bench/phase0/parity.sh [BINARY] [OUT_DIR]
# Every analyzer run holds /tmp/rust-dialyzer-run.lock: one run peaks at
# 15-20 GB and the machine has 36 GB, so concurrent runs get OOM-killed.
set -euo pipefail
cd "$(dirname "$0")/../.."
B=${1:-target/perf/release/core_cache_analyze}; O=${2:-/tmp/parity}; mkdir -p "$O"
S=.cache/phase0/phoenix-scenario; P=.cache/phase0/rust-cone/out/prior-library.json
L() { lockf -k /tmp/rust-dialyzer-run.lock "$@"; }
echo "== phoenix"
L /usr/bin/time -l "$B" $P $S/manifest.json $S/options.json > "$O/p.json" 2> "$O/p.err"
grep -E "real|instructions retired|peak memory" "$O/p.err"
python3 bench/phase0/compare_warnings.py $S/otp-check-warnings.json "$O/p.json" | head -1
python3 -c "import json;r=json.load(open('$O/p.json'));open('$O/p-sigs.jsonl','w').write(json.dumps({'signatures':r['signatures']})+'\n')"
python3 reports/real-projects/fixes/inferred-signatures/compare.py "$O/p-sigs.jsonl" $S/oracle/otp-full-signatures.json --out "$O/sig.json" | tail -1 \
  | python3 -c "import json,sys;d=json.loads(sys.stdin.read());print('sigs',d['exact_matches'],'/',d['otp_signature_count'],'parity',d['complete_exact_parity']);assert d['complete_exact_parity']"
python3 bench/phase0/output_hash.py "$O/p.json"
echo "== phoenix verify"
RUST_DIALYZER_VERIFY_FASTPATH=1 L "$B" $P $S/manifest.json $S/options.json > "$O/pv.json" 2> "$O/pv.err"
echo "exit $? panics $(grep -ci panic "$O/pv.err")"; python3 bench/phase0/output_hash.py "$O/pv.json"
F=".cache/phase0/core-cache/manifest-etf.json .cache/phase0/core-cache/oracle-full-unknown"
U=".cache/phase0/subset/manifest.json .cache/phase0/subset/oracle"
echo "== full";        L python3 bench/phase0/check.py "$B" $F "$O/full" | grep -E "instructions|peak|warnings|signatures|parity"
echo "== full verify"; L python3 bench/phase0/check.py "$B" $F "$O/fullv" --env RUST_DIALYZER_VERIFY_FASTPATH=1 | grep -E "warnings|signatures|parity"
echo "== subset";      L python3 bench/phase0/check.py "$B" $U "$O/sub" | grep -E "warnings|signatures|parity"
echo "== subset seq";  L python3 bench/phase0/check.py "$B" $U "$O/subs" --env RUST_DIALYZER_SEQUENTIAL=1 | grep -E "warnings|signatures|parity"
