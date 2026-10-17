#!/usr/bin/env python3
"""Verify atomic/scoped constraints against the selected original OTP runtime."""
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts/constraint_probes'))
from config import configure, parser

GATES = [('atomic', 12800), ('tree', 2002), ('references', 1610), ('scc', 401),
         ('derived', 22528), ('dynamic', 2251), ('structural', 10150),
         ('normalize', 2007), ('preparation', 1001), ('prepared_solve', 2000),
         ('remaining_derived', 3018), ('contract_derived', 1000)]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def compress_outputs(out):
    for path in out.iterdir():
        if not path.is_file() or path.suffix not in ('.json', '.jsonl', '.erl'):
            continue
        if path.name.endswith('verification.json') or path.name in ('report.json', 'closure-metadata.json', 'closure-code-profile.json'):
            continue
        with path.open('rb') as source, gzip.open(str(path) + '.gz', 'wb', compresslevel=6) as target:
            shutil.copyfileobj(source, target)
        path.unlink()


def main():
    arguments = parser()
    arguments.description = __doc__
    arguments.add_argument('--compress-artifacts', action='store_true')
    config = configure(arguments.parse_args())
    out = config.out
    files = [*sorted((ROOT / 'src/constraints').rglob('*.rs')), ROOT / 'src/lattice.rs',
             *sorted((ROOT / 'src/lattice').rglob('*.rs')), ROOT / 'src/contract_runtime.rs',
             ROOT / 'src/core.rs', ROOT / 'src/term_order.rs', ROOT / 'src/type_api.rs',
             ROOT / 'src/native_order.rs', ROOT / 'src/runtime_term.rs', ROOT / 'src/presentation.rs',
             ROOT / 'src/presentation_form.rs', ROOT / 'src/bin/constraints_probe.rs',
             *sorted((ROOT / 'src/bifs').glob('*.rs')), ROOT / 'corpus/bifs.json',
             ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'scripts/type_codec.exs',
             ROOT / 'scripts/type_wire.exs',
             *sorted((ROOT / 'scripts/constraint_probes').iterdir()), Path(__file__).resolve()]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files if p.is_file() and p.name != ".DS_Store"}
    if not config.binary_explicit:
        if config.otp != '28':
            raise ValueError('OTP29 requires an explicitly built --binary')
        subprocess.run(['cargo', 'build', '--release', '--offline', '--bin', 'constraints_probe'], cwd=ROOT, check=True)
    binary_hash = digest(config.binary)
    source_hash = digest(config.source_file)
    reports = []
    for gate, count in GATES:
        subprocess.run([sys.executable, str(ROOT / f'scripts/constraint_probes/verify_{gate}.py')], cwd=ROOT, env=config.env, check=True)
        report = json.loads((out / f'{gate.replace("_", "-")}-verification.json').read_text())
        reports.append(dict(gate=gate, **report))
        if config.compress_artifacts:
            compress_outputs(out)
        if report['commands'] != count or report['passed'] != count or report['failures']:
            raise RuntimeError(f'{gate}: full native parity not achieved ({report["passed"]}/{count})')
        if digest(config.binary) != binary_hash or digest(config.source_file) != source_hash:
            raise RuntimeError('probe or native source changed during verification')
    subprocess.run([sys.executable, str(ROOT / 'scripts/constraint_probes/verify_closures.py')], cwd=ROOT, env=config.env, check=True)
    closures = json.loads((out / 'closure-verification.json').read_text())
    expected_sites = 55 if config.otp == '28' else closures['expected_code_sites']
    expected_factory = 64 if config.otp == '28' else closures['expected_factory_cases']
    if (closures['pairs'] != 6561 or closures['passed'] != 6561 or closures['differences']
            or closures['factory_cases'] != expected_factory or closures['code_sites'] != expected_sites
            or closures['otp'] != config.otp):
        raise RuntimeError('closure code profile or ordering parity changed')
    if hashes != {name: digest(ROOT / name) for name in hashes}:
        raise RuntimeError('constraint implementation changed during verification')
    metadata = json.loads((out / 'closure-metadata.json').read_text())
    if digest(config.binary) != binary_hash or digest(config.source_file) != source_hash:
        raise RuntimeError('probe or native source changed during verification')
    report = dict(otp=config.otp, complete_atomic_and_scoped_constraints=True,
        complete_success_typing=False,
        unsupported=['record phase snapshots', 'ordered derived normalization and captured snapshots', 'Core generation', 'production success phase'],
        implementation_sha256=hashes,
        inputs_sha256={str(config.source_file): source_hash, metadata['beam']: digest(Path(metadata['beam'])), str(config.binary): binary_hash},
        commands=sum(r['commands'] for r in reports), passed=sum(r['passed'] for r in reports),
        gates=reports, closure_ordering=closures)
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    if config.compress_artifacts:
        compress_outputs(out)
        for name in ('closure-metadata.json', 'closure-code-profile.json'):
            path = out / name
            with path.open('rb') as source, gzip.open(str(path) + '.gz', 'wb') as target:
                shutil.copyfileobj(source, target)
            path.unlink()
    print(json.dumps({k: report[k] for k in ('otp', 'commands', 'passed', 'complete_success_typing')}))


if __name__ == '__main__':
    main()
