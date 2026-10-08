#!/usr/bin/env python3
"""Compare owned prior values and exact boundary errors through two libraries."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess

p = argparse.ArgumentParser(description=__doc__)
for name in ['baseline', 'candidate', 'core', 'prior', 'out']:
    p.add_argument('--' + name, type=Path, required=True)
a = p.parse_args()
a.out.mkdir(parents=True, exist_ok=False)
source = Path(__file__).with_name('prior_owned_boundary_probe.rs')
fixture = json.loads(a.prior.read_text())
minimal = copy.deepcopy(fixture)
nil = dict(root=0, terms=[dict(kind='nil')])
arenas = ['raw_summaries', 'raw_contracts', 'raw_callback_groups', 'type_tables', 'exported_types']
for name in arenas:
    minimal[name] = copy.deepcopy(nil)
cases = [fixture, minimal, None, [], 'bad', 1, {}]
for name in arenas:
    for value in [None, False, {}, dict(root=2, terms=[dict(kind='nil')])]:
        row = copy.deepcopy(minimal)
        row[name] = value
        cases.append(row)
    row = copy.deepcopy(minimal)
    del row[name]
    cases.append(row)
for name in ['version', 'profile', 'source', 'modules', 'summaries', 'contracts', 'callback_lists', 'callbacks']:
    row = copy.deepcopy(minimal)
    row[name] = False
    cases.append(row)
    row = copy.deepcopy(minimal)
    row.pop(name, None)
    cases.append(row)
for version in [3, 4, 6]:
    row = copy.deepcopy(minimal)
    row['version'] = version
    cases.append(row)
data = ('\n'.join(json.dumps(x) for x in cases) + '\n').encode()
(a.out / 'inputs.jsonl').write_bytes(data)
paths = [source, Path(__file__), a.core, a.prior]
libraries = {}
for label, root in [('baseline', a.baseline), ('candidate', a.candidate)]:
    deps = root / 'target/release/deps'
    lib = list(deps.glob('librust_dialyzer_poc-*.rlib'))
    serde = list(deps.glob('libserde_json-*.rlib'))
    assert len(lib) == len(serde) == 1
    paths += lib + serde
    libraries[label] = (deps, lib[0], serde[0])
inventory = lambda: {str(x): hashlib.sha256(x.read_bytes()).hexdigest() for x in paths}
before = inventory()
outputs = []
for label, (deps, library, serde) in libraries.items():
    binary = a.out / label
    subprocess.run(['rustc', '--edition=2024', str(source), '-L', 'dependency=' + str(deps),
                    '--extern', 'rust_dialyzer_poc=' + str(library), '--extern', 'serde_json=' + str(serde),
                    '-o', str(binary)], check=True)
    run = subprocess.run([str(binary), str(a.core)], input=data, capture_output=True, check=True)
    (a.out / (label + '.jsonl')).write_bytes(run.stdout)
    outputs.append(run.stdout)
assert outputs[0] == outputs[1], 'owned prior values or exact errors differ'
rows = [json.loads(x) for x in outputs[0].splitlines()]
assert 'ok' in rows[0] and 'ok' in rows[1], 'real and minimal prior must succeed'
assert inventory() == before
report = dict(cases=len(rows), valid=sum('ok' in r for r in rows), errors=sum('error' in r for r in rows),
              outcomes_and_errors_exact=True, inputs_unchanged=True, input_sha256=before)
(a.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({k: v for k, v in report.items() if k != 'input_sha256'}))
