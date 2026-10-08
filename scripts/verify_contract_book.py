#!/usr/bin/env python3
"""Compare target-contract activation and persistence with the native phases."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.cache/contract_book'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    source = ROOT / '.cache/contracts/augmented_contracts.json'
    contracts = json.loads(source.read_text())['contracts']
    if len(contracts) != 3243:
        raise RuntimeError('contract activation source denominator changed')
    files = [ROOT / 'src/contract_book.rs', ROOT / 'src/validation.rs', ROOT / 'src/contract_runtime.rs', ROOT / 'src/dependencies.rs', ROOT / 'src/bifs/registry.rs', ROOT / 'src/bifs/boundary.rs', ROOT / 'src/lattice.rs', ROOT / 'src/type_api.rs', ROOT / 'src/native_order.rs', ROOT / 'src/runtime_term.rs', ROOT / 'src/presentation.rs', ROOT / 'src/presentation_form.rs', ROOT / 'src/bin/contract_book_probe.rs', ROOT / 'scripts/type_codec.exs', ROOT / 'scripts/contract_book_oracle.exs', ROOT / 'corpus/bifs.json', ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', Path(__file__).resolve()]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files}
    inputs = {str(source.relative_to(ROOT)): digest(source)}
    subprocess.run(['cargo', 'build', '--release', '--offline', '--bin', 'contract_book_probe'], cwd=ROOT, check=True)
    kinds = [('any', 'any'), ('any', 'none'), ('any', 'unit'), ('any', 'integer'), ('integer', 'atom'), ('atom', 'integer'), ('none', 'any'), ('unit', 'none'), ('nil', 'nil'), ('integer', 'integer')]
    requests = []
    for index, contract in enumerate(contracts):
        baseline = dict(kind='function', domain=[dict(kind='any')] * contract['mfa']['arity'], range=dict(kind='any'))
        for i, (arg, ret) in enumerate(kinds):
            other = dict(kind='function', domain=[dict(kind=arg)] * contract['mfa']['arity'], range=dict(kind=ret))
            requests.append(dict(contract_index=index, successes=[other, baseline] if i % 2 else [baseline, other]))
    data = ''.join(json.dumps(r) + '\n' for r in requests)
    request_file = OUT / 'commands.jsonl'
    request_file.write_text(data)
    outputs = {}
    for name, command in [('rust', [str(ROOT / 'target/release/contract_book_probe'), str(source)]), ('native', ['elixir', str(ROOT / 'scripts/contract_book_oracle.exs'), str(source), str(request_file)])]:
        result = subprocess.run(command, input=data if name == 'rust' else None, capture_output=True, text=True, cwd=ROOT, check=True)
        (OUT / f'{name}.jsonl').write_text(result.stdout)
        outputs[name] = [json.loads(row) for row in result.stdout.splitlines()]
        if len(outputs[name]) != len(requests):
            raise RuntimeError(f'{name} activation denominator changed')
    failures = [dict(index=i, request=requests[i], rust=rust, native=native) for i, (rust, native) in enumerate(zip(outputs['rust'], outputs['native'])) if rust != native]
    if hashes != {name: digest(ROOT / name) for name in hashes} or inputs != {name: digest(ROOT / name) for name in inputs}:
        raise RuntimeError('contract activation implementation or inputs changed during verification')
    report = dict(complete_contract_activation=not failures, complete_contract_orchestration=False, contracts=len(contracts), commands=len(requests), passed=len(requests)-len(failures), failures=failures, implementation_sha256=hashes, inputs_sha256=inputs)
    (OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: report[k] for k in ('contracts', 'commands', 'passed', 'complete_contract_activation')}))
    if failures:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
