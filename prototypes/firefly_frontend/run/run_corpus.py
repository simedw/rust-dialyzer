"""Exercise native frontend and existing analyzer with explicit frozen oracle profiles."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

base = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('repo', type=Path)
parser.add_argument('--binary-root', type=Path, help='Use separately built prototype binaries')
args = parser.parse_args()
repo = args.repo.resolve()
binary_root = args.binary_root.resolve() if args.binary_root else base
harness = base/'harness'
output = base/'corpus-native'
output.mkdir(exist_ok=True)
vm_trap = output/'vm-trap'
vm_trap.mkdir(exist_ok=True)
trap_log = output/'unexpected-vm.log'
for name in ('erl', 'elixir', 'escript', 'mix'):
    path = vm_trap/name
    path.write_text('#!/bin/sh\nprintf "%s\\n" "$0" >> "$NATIVE_VM_TRAP_LOG"\nexit 97\n')
    path.chmod(0o755)
environment = dict(os.environ, PATH=str(vm_trap), NATIVE_VM_TRAP_LOG=str(trap_log))
seed_binary = binary_root/'input/target/debug/native-beam-seed'
adapter_binary = binary_root/'adapter/target/debug/adapt-beam'
analyzer_binary = repo/'target/debug/compiled_analysis_probe'
expected = [json.loads(line) for line in (harness/'corpus/oracle-row.jsonl').read_text().splitlines()]
native_rows = []
failures = []
for row in expected:
    module_name = row['core']['modules'][0]['name']
    seed = output/f'{module_name}.seed.json'
    module = output/f'{module_name}.module.json'
    beam = row['beam_path']
    for command in ([seed_binary, beam, seed], [adapter_binary, beam, seed, module]):
        process = subprocess.run([str(x) for x in command], capture_output=True, text=True,
                                 env=environment, timeout=30)
        if process.returncode:
            failures.append(dict(id=row['id'], command=[str(x) for x in command], error=process.stderr))
            break
    else:
        # Only captured runtime profiles are carried over. All executable
        # nodes, attributes, and source forms are native-produced.
        core = {k: row['core'][k] for k in ('version', 'profile', 'atom_layout', 'contract_closure_profile')}
        core['modules'] = [json.loads(module.read_text())]
        row = dict(row, core=core)
        native_rows.append(row)
        print(f'Native frontend: {module_name}', flush=True)
(output/'native-rows.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in native_rows))
with (output/'native-rows.jsonl').open() as incoming, (output/'analyzer-results.jsonl').open('w') as outgoing:
    subprocess.run([str(analyzer_binary), str(harness/'prior-library.json'), str(harness/'closure-profile.json')],
                   stdin=incoming, stdout=outgoing, env=environment, check=True, timeout=180)
baseline = {row['id']: row for row in map(json.loads, (harness/'corpus/oracle-rust-result.jsonl').read_text().splitlines())}
actual = [json.loads(line) for line in (output/'analyzer-results.jsonl').read_text().splitlines()]
assert len(actual) == len(native_rows)
checks = []
for row, result in zip(native_rows, actual):
    passed = result.get('public_warnings_match') is True and result.get('public_warning_count') == row['warning_count']
    signatures = lambda value: sorted(value.get('signatures', []), key=lambda item: (item['module'], item['function'], item['arity']))
    signature_match = signatures(result) == signatures(baseline[row['id']])
    subprocess.run([sys.executable, str(base/'input/verify_metadata.py'), str(harness/'corpus'/row['module']), str(output/f"{row['module']}.seed.json")], check=True, stdout=subprocess.DEVNULL)
    checks.append(dict(id=row['id'], warnings=row['warning_count'], passed=passed, signatures_match=signature_match, metadata_match=True, actual=result))
    if not signature_match:
        failures.append(dict(id=row['id'], phase='inferred_signatures'))
    print(f"Warning comparison: {row['id']} {'PASS' if passed else 'FAIL'}", flush=True)
    if not passed:
        failures.append(dict(id=row['id'], phase='analyzer', actual=result))
assert not trap_log.exists(), 'native path attempted VM subprocess'
paths = [seed_binary, adapter_binary, analyzer_binary, harness/'prior-library.json', harness/'closure-profile.json',
         harness/'corpus/oracle-row.jsonl', Path(__file__).resolve()]
paths.extend(Path(row['beam_path']) for row in expected)
report = dict(cases=len(expected), native_frontend_passed=len(native_rows),
              exact_warning_matches=sum(c['passed'] for c in checks), inferred_signature_matches=sum(c['signatures_match'] for c in checks), metadata_matches=sum(c['metadata_match'] for c in checks), failures=failures,
              checks=checks, vm_trap_invoked=False, captured_runtime_profiles=True,
              oracle_bodies_as_native_input=False, scope='Erlang fixtures only; no performance or Elixir parity claim',
              sha256={str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in paths})
(output/'report.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k not in ('checks','sha256','failures')}))
sys.exit(1 if failures else 0)
