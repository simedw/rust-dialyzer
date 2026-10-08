#!/usr/bin/env python3
"""Compare all imported records and demanded signatures through both built worlds."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--baseline', type=Path, required=True)
p.add_argument('--candidate', type=Path, required=True)
p.add_argument('--core', type=Path, required=True)
p.add_argument('--prior', type=Path, required=True)
p.add_argument('--out', type=Path, required=True)
a = p.parse_args()
a.out.mkdir(parents=True, exist_ok=False)
paths = [Path(__file__), a.core, a.prior]
for root in [a.baseline, a.candidate]:
    paths += list((root / 'src').rglob('*.rs'))
    paths += [root / 'target/release' / name for name in ['value_contracts_probe', 'value_signatures_probe']]
inventory = lambda: {str(x): hashlib.sha256(x.read_bytes()).hexdigest() for x in paths}
before = inventory()
results = {}
for table, binary in [('contracts', 'value_contracts_probe'), ('signatures', 'value_signatures_probe')]:
    outputs = []
    for label, root in [('baseline', a.baseline), ('candidate', a.candidate)]:
        run = subprocess.run([str(root / 'target/release' / binary), str(a.core), str(a.prior)],
                             capture_output=True, check=True, timeout=600)
        (a.out / f'{label}-{table}.stdout').write_bytes(run.stdout)
        (a.out / f'{label}-{table}.stderr').write_bytes(run.stderr)
        outputs.append((run.stdout, run.stderr))
    assert outputs[0] == outputs[1], (table, 'values or exhaustive lookup counts differ')
    results[table] = dict(rows=len(outputs[0][0].splitlines()),
                         sha256=hashlib.sha256(outputs[0][0]).hexdigest(), baseline_exact=True)
assert results['contracts']['rows'] == 3314 and results['signatures']['rows'] == 5407
assert inventory() == before, 'source, binary or input changed'
report = dict(results=results, inputs_unchanged=True, input_sha256=before)
(a.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(results))
