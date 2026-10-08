#!/usr/bin/env python3
"""Check coherent guard transfer and raw diagnostics against original OTP."""
import hashlib
import itertools
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.cache/guard_full'
OUT.mkdir(parents=True, exist_ok=True)
suites = [
    ('control', 'guard_control_oracle.exs', 34800),
    ('calls', 'guard_calls_oracle.exs', 19980),
    ('booleans', 'guard_booleans_oracle.exs', 41262),
    ('equality', 'guard_equality_oracle.exs', 139656),
    ('shapes', 'guard_shapes_oracle.exs', 139806),
    ('cases', 'guard_case_oracle.exs', 38088),
]
files = list((ROOT / 'src').rglob('*.rs')) + [ROOT / p for p in (
    'Cargo.toml', 'Cargo.lock', 'corpus/bifs.json', 'scripts/verify_guard_full.py',
    'scripts/core_export.exs', 'scripts/type_codec.exs', 'scripts/type_wire.exs',
    'scripts/pattern_probes/binder_oracle.exs',
)] + [ROOT / 'scripts/guard_probes' / s for _, s, _ in suites]
files.append(ROOT / 'scripts/guard_probes/guard_divergence_oracle.exs')
files.extend(Path(p) for p in subprocess.check_output([
    'elixir', '-e', 'Enum.each([:dialyzer_dataflow,:erl_types,:erl_bif_types,:cerl,:cerl_trees],fn m->IO.puts(:code.which(m))end)',
], cwd=ROOT, text=True).splitlines())


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def hashes():
    return {str(path): sha(path) for path in files}


before = hashes()
subprocess.run(['cargo', 'build', '--offline', '--release', '--bin', 'guard_control_probe'], cwd=ROOT, check=True)
binary = ROOT / 'target/release/guard_control_probe'
inputs = {str(binary): sha(binary)}
gates = {}
for suite, script, count in suites:
    native = OUT / f'{suite}.native.jsonl'
    actual = OUT / f'{suite}.rust.jsonl'
    with native.open('wb') as output:
        subprocess.run(['elixir', str(ROOT / 'scripts/guard_probes' / script)], cwd=ROOT, stdout=output, check=True)
    inputs[str(native)] = sha(native)
    with native.open() as source:
        for line in source:
            row = json.loads(line)
            path = row.get('core_json')
            if path and path not in inputs:
                inputs[path] = sha(Path(path))
    with native.open('rb') as source, actual.open('wb') as output:
        subprocess.run([str(binary), str(ROOT / 'corpus/bifs.json')], cwd=ROOT, stdin=source, stdout=output, check=True)
    gate = dict(cases=0, native_errors=0, failures=0, returned_warnings=0, raw_warning_logs=0)
    failures = []
    with native.open() as source, actual.open() as result:
        for left, right in itertools.zip_longest(source, result):
            assert left is not None and right is not None, 'partial guard output'
            n, r = json.loads(left), json.loads(right)
            gate['cases'] += 1
            raw = 'warning_terms' in n.get('ok', {})
            if n['id'] != r['id'] or not r['matches'] or (raw and not r['warning_exact']):
                if len(failures) < 20:
                    failures.append(r)
            gate['native_errors'] += 'error' in n
            gate['failures'] += 'failure' in n
            gate['returned_warnings'] += len(n.get('ok', {}).get('warnings', []))
            gate['raw_warning_logs'] += raw
    (OUT / f'{suite}.failures.json').write_text(json.dumps(failures, indent=2) + '\n')
    assert gate['cases'] == count and not failures, (suite, gate, failures[:1])
    gates[suite] = gate
    print(json.dumps({suite: gate}), flush=True)
divergence = OUT / 'divergence.native.jsonl'
with divergence.open('wb') as output:
    subprocess.run(['elixir', str(ROOT / 'scripts/guard_probes/guard_divergence_oracle.exs')], cwd=ROOT, stdout=output, check=True)
observations = [json.loads(line) for line in divergence.read_text().splitlines()]
assert len(observations) == 6 and all(o['observation'] == 'deadline_in_number_vals' for o in observations)
inputs[str(divergence)] = sha(divergence)
assert before == hashes()
assert all(sha(Path(path)) == digest for path, digest in inputs.items())
report = dict(cases=413592, gates=gates, native_nontermination_observations=observations,
              complete_guard_interpreter=False, complete_module_inference=False,
              remaining=['malformed private Core getter payloads',
                         'arbitrary Try throw payloads', 'complete module inference'],
              implementation_sha256=before, inputs_sha256=inputs)
(OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({'cases': report['cases'], 'complete_guard_interpreter': False}))
