#!/usr/bin/env python3
"""Verify the shared atomic and scoped solver before Core phase integration."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.cache/constraints'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    files = [*sorted((ROOT / 'src/constraints').glob('*.rs')), ROOT / 'src/lattice.rs', ROOT / 'src/contract_runtime.rs', ROOT / 'src/core.rs', ROOT / 'src/term_order.rs', ROOT / 'src/type_api.rs', ROOT / 'src/native_order.rs', ROOT / 'src/runtime_term.rs', ROOT / 'src/presentation.rs', ROOT / 'src/presentation_form.rs', ROOT / 'src/bin/constraints_probe.rs', *sorted((ROOT / 'src/bifs').glob('*.rs')), ROOT / 'corpus/bifs.json', ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'scripts/type_codec.exs', ROOT / 'scripts/type_wire.exs', *sorted((ROOT / 'scripts/constraint_probes').iterdir()), Path(__file__).resolve()]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files if p.is_file()}
    subprocess.run(['cargo', 'build', '--release', '--offline', '--bin', 'constraints_probe'], cwd=ROOT, check=True)
    reports = []
    for gate, count in [('atomic', 12800), ('tree', 2002), ('references', 1610), ('scc', 401), ('derived', 22528), ('dynamic', 2251), ('structural', 10150), ('normalize', 2007), ('preparation', 1001), ('prepared_solve', 2000), ('remaining_derived', 3018), ('contract_derived', 1000)]:
        subprocess.run([sys.executable, str(ROOT / f'scripts/constraint_probes/verify_{gate}.py')], cwd=ROOT, check=True)
        report = json.loads((OUT / f'{gate.replace('_', '-')}-verification.json').read_text())
        if report['commands'] != count or report['passed'] != count or report['failures']:
            raise RuntimeError(f'{gate}: full native parity not achieved')
        reports.append(dict(gate=gate, **report))
    subprocess.run([sys.executable, str(ROOT / 'scripts/constraint_probes/verify_closures.py')], cwd=ROOT, check=True)
    closures = json.loads((OUT / 'closure-verification.json').read_text())
    if closures['pairs'] != 6561 or closures['passed'] != 6561 or closures['differences'] or closures['factory_cases'] != 64 or closures['code_sites'] != 55:
        raise RuntimeError('closure code profile or ordering parity changed')
    if hashes != {name: digest(ROOT / name) for name in hashes}:
        raise RuntimeError('constraint implementation changed during verification')
    native_source = ROOT / '.cache/upstream/otp/lib/dialyzer/src/dialyzer_typesig.erl'
    report = dict(complete_atomic_and_scoped_constraints=True, complete_success_typing=False, unsupported=['record phase snapshots', 'ordered derived normalization and captured snapshots', 'Core generation', 'production success phase'], implementation_sha256=hashes, inputs_sha256={str(native_source.relative_to(ROOT)): digest(native_source), str(json.loads((OUT / 'closure-metadata.json').read_text())['beam']): digest(Path(json.loads((OUT / 'closure-metadata.json').read_text())['beam']))}, commands=sum(r['commands'] for r in reports), passed=sum(r['passed'] for r in reports), gates=reports, closure_ordering=closures)
    (OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: report[k] for k in ('commands', 'passed', 'complete_success_typing')}))


if __name__ == '__main__':
    main()
