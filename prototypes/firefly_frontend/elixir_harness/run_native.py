"""Verify native Elixir translation without granting access to a VM executable."""
import argparse
from functools import lru_cache
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from native_requests import request

parser = argparse.ArgumentParser()
parser.add_argument('repo', type=Path)
parser.add_argument('--native-bin', type=Path)
parser.add_argument('--case', action='append', default=[])
args = parser.parse_args()
base = Path(__file__).resolve().parent
prototype = base.parent
repo = args.repo.resolve()
oracle_dir = base/'generated/oracle'
out = base/'generated/native'
out.mkdir(parents=True,exist_ok=True)
trap = out/'vm-trap'
trap.mkdir(exist_ok=True)
log = out/'unexpected-vm.log'
if log.exists():
    log.unlink()
for name in ('erl','elixir','escript','mix'):
    executable = trap/name
    executable.write_text('#!/bin/sh\nprintf "%s\\n" "$0" >> "$NATIVE_VM_TRAP_LOG"\nexit 97\n')
    executable.chmod(0o755)
environment = dict(os.environ,PATH=str(trap),NATIVE_VM_TRAP_LOG=str(log))
native_binary = args.native_bin.resolve() if args.native_bin else repo/'target/release/native-elixir-dialyzer'
rows = [json.loads(line) for line in (oracle_dir/'oracle-rows.jsonl').read_text().splitlines()]
if args.case:
    rows = [r for r in rows if r['id'] in args.case]
baseline = {r['id']:r for r in map(json.loads,(oracle_dir/'baseline-results.jsonl').read_text().splitlines())}

def decoder(module):
    terms = module['terms']
    @lru_cache(None)
    def at(i):
        t = terms[i]
        k = t['kind']
        if k in ('atom','integer','float','binary'):
            return k,t['value']
        if k == 'bitstring':
            return k,t['bytes'],t['bits']
        if k == 'nil':
            return 'list',(),None
        if k == 'tuple':
            return k,tuple(at(x) for x in t['items'])
        if k == 'list':
            tail = at(t['tail'])
            items = tuple(at(x) for x in t['items'])
            return ('list',items+tail[1],tail[2]) if tail[0]=='list' else ('list',items,tail)
        if k == 'map':
            return k,tuple(sorted((at(a),at(b)) for a,b in t['entries']))
        raise ValueError(k)
    return at

def attributes(module):
    d = decoder(module)
    root = module['nodes'][module['core']]
    result = []
    for a,b in zip(root['groups'][2][::2],root['groups'][2][1::2]):
        name,value = module['nodes'][a],module['nodes'][b]
        result.append((d(name['value']),d(value['value']),d(value['annotations'])))
    return result

module_checks = {}
failures = []
checks = []
requests = [request(row, emit_core=True) for row in rows]
for row in rows:
    for beam, digest in row['beam_sha256'].items():
        assert hashlib.sha256(Path(beam).read_bytes()).hexdigest() == digest
(out/'requests.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in requests))
process = subprocess.run([str(native_binary), str(oracle_dir/'prior-library.json'),
                          str(oracle_dir/'closure-profile.json')],
                         input=''.join(json.dumps(r)+'\n' for r in requests),
                         capture_output=True, text=True, env=environment, timeout=180)
actual = [json.loads(line) for line in process.stdout.splitlines()]
assert len(actual) == len(rows), process.stderr or process.stdout
(out/'results.jsonl').write_text(process.stdout)
complete = 0
for row, result in zip(rows, actual):
    if 'error' in result:
        failures.append(dict(id=row['id'], phase='merged_pipeline', error=result['error']))
        continue
    complete += 1
    native_modules = {m['name']: m for m in result['core']['modules']}
    oracle_modules = {m['name']: m for m in row['core']['modules']}
    assert set(native_modules) == set(oracle_modules)
    for name, native in native_modules.items():
        reference = oracle_modules[name]
        forms_equal = decoder(native)(native['source_forms']) == decoder(reference)(reference['source_forms'])
        attrs_equal = attributes(native) == attributes(reference)
        if not (forms_equal and attrs_equal):
            failures.append(dict(id=row['id'], module=name, phase='source_metadata',
                                 forms_match=forms_equal, attributes_match=attrs_equal))
        module_checks[name] = dict(raw_source_forms_structurally_equal=forms_equal,
                                  source_attributes_and_locations_equal=attrs_equal)
    expected = baseline[row['id']]
    signatures = lambda value: sorted(value.get('signatures', []), key=lambda v:(v['module'],v['function'],v['arity']))
    # Warnings are compared outside the native process, never supplied as inputs.
    warnings = lambda value: decoder(value['warnings'])(value['warnings']['root'])
    warning_match = warnings(result) == decoder(row['warnings'])(row['warnings']['root']) and result['public_warning_count'] == row['warning_count']
    signature_match = signatures(result) == signatures(expected)
    checks.append(dict(id=row['id'], warnings_match=warning_match, inferred_types_match=signature_match,
                       expected_warnings=row['warning_count']))
    if not (warning_match and signature_match):
        failures.append(dict(id=row['id'], phase='analysis', warnings_match=warning_match, inferred_types_match=signature_match))
    print(row['id']+': '+('passed' if warning_match and signature_match else 'failed'), flush=True)
assert not log.exists(), 'native frontend invoked VM command'
assert process.returncode == 0, process.stderr
report = dict(cases=len(rows), native_frontend_passed=complete,
              exact_warning_matches=sum(c['warnings_match'] for c in checks),
              inferred_type_matches=sum(c['inferred_types_match'] for c in checks),
              checks=checks, module_metadata=module_checks, failures=failures,
              native_path_launched_vm=False, captured_runtime_profiles=True,
              oracle_core_bodies_used_as_frontend_input=False, oracle_warnings_used_as_native_input=False,
              native_process_count=1, intermediate_frontend_files=False,
              sha256={str(native_binary):hashlib.sha256(native_binary.read_bytes()).hexdigest()})
(out/'report.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k not in ('checks','module_metadata','failures','sha256')}))
sys.exit(1 if failures else 0)
