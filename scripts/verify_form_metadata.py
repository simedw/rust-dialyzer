"""Generate native type/contract references and compare every case in an engine manifest."""
import argparse, hashlib, json, subprocess
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--manifest', required=True, type=Path)
parser.add_argument('--probe', required=True, type=Path)
parser.add_argument('--output', required=True, type=Path)
args = parser.parse_args()
script_dir = Path(__file__).resolve().parent
root = script_dir.parent
implementation_files = [*sorted((root / 'src').rglob('*.rs')), root / 'Cargo.toml', root / 'Cargo.lock',
                        *[script_dir / name for name in ['verify_form_metadata.py', 'forms_oracle.exs',
                          'local_contracts_oracle.exs', 'validation_oracle.exs', 'forms_runtime.exs',
                          'native_form_metadata.exs', 'core_export.exs', 'native_types.exs']]]
implementation = {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in implementation_files}

probe = args.probe.resolve()
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=True)
manifest = json.loads(args.manifest.read_text())
if len({row['id'] for row in manifest}) != len(manifest):
    raise SystemExit('manifest contains duplicate case IDs')

def write_rows(path, rows):
    path.write_text(''.join(json.dumps(row) + '\n' for row in rows))

def read_rows(path, count):
    rows = [json.loads(line) for line in path.read_text().splitlines()]
    if len(rows) != count:
        raise SystemExit(f'{path}: expected {count} complete rows, got {len(rows)}')
    return rows

def native(script, requests, name):
    request_path = output / (name + '.requests.jsonl')
    reference_path = output / (name + '.native.jsonl')
    write_rows(request_path, requests)
    with request_path.open() as input_file, reference_path.open('w') as result:
        subprocess.run(['elixir', str(script_dir / script)], stdin=input_file, stdout=result, check=True)
    return read_rows(reference_path, len(requests))

def compare(operation, requests, references, name, keys):
    path = output / (name + '.compare.requests.jsonl')
    result_path = output / (name + '.results.jsonl')
    write_rows(path, [dict(core=req['core'], environment=req['environment'], reference=ref)
                      for req, ref in zip(requests, references)])
    with path.open() as input_file, result_path.open('w') as result:
        subprocess.run([str(probe), '--' + operation], stdin=input_file, stdout=result, check=True)
    results = read_rows(result_path, len(requests))
    summary = {key: sum(row[key] for row in results) for key in keys}
    summary['cases'] = len(results)
    (output / (name + '.summary.json')).write_text(json.dumps(summary, sort_keys=True) + '\n')
    print(name, json.dumps(summary, sort_keys=True), flush=True)
    if summary['failed']:
        raise SystemExit(f'{name}: native comparisons failed')
    return summary

requests = []
for case in manifest:
    inspection = json.loads(subprocess.check_output([str(probe), case['core_json']], text=True))
    requests.append(dict(case=case['id'], core=case['core_json'], nodes=inspection['nodes'],
                         inspection=inspection['inspection'], environment=case['dependency_json']))
references = native('forms_oracle.exs', requests, 'environment_forms')
forms_summary = compare('compare', requests, references, 'environment_forms',
                        ['roots', 'matched', 'native_errors', 'missing_events', 'failed'])
contract_requests = [dict(case=req['case'], core=req['core'], environment=req['environment'], finalized=True)
                     for req in requests]
contracts = native('local_contracts_oracle.exs', contract_requests, 'finalized_contracts')
contracts_summary = compare('local-contracts', contract_requests, contracts, 'finalized_contracts',
                            ['contracts', 'record_fields', 'passed', 'native_errors', 'failed'])
validation_requests = [dict(core=case['core_json'], environment=case['dependency_json'],
                            facts=case['native_json'], reference=reference)
                       for case, reference in zip(manifest, contracts)]
validation = native('validation_oracle.exs', validation_requests, 'finalized_validation')
validation_summary = compare('validation', validation_requests, validation, 'finalized_validation',
                             ['compared', 'matched', 'missing_success', 'native_errors', 'failed'])
paths = sorted({str(Path(case[key]).resolve()) for case in manifest
                for key in ['core_json', 'dependency_json', 'native_json']})
provenance = {path: hashlib.sha256(Path(path).read_bytes()).hexdigest() for path in paths}
(output / 'source_hashes.json').write_text(json.dumps(provenance, indent=2) + '\n')
(output / 'summary.json').write_text(json.dumps(dict(forms=forms_summary, contracts=contracts_summary,
                                                    validation=validation_summary), indent=2) + '\n')

if implementation != {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in implementation_files}:
    raise SystemExit('implementation changed during metadata verification')
(output / 'report.json').write_text(json.dumps(dict(complete=False, complete_valid_metadata=True,
    exception_payloads_verified=False, all_private_successes_verified=False,
    implementation_sha256=implementation, inputs_sha256=provenance,
    forms=forms_summary, contracts=contracts_summary, validation=validation_summary), indent=2)+'\n')
