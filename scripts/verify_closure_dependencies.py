#!/usr/bin/env python3
"""Compare the production closure graph with the native pass for every Core input."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.cache/closure_dependencies'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def union(a, b):
    return {'external': a['external'] or b['external'], 'local': sorted(set(a['local']) | set(b['local']))}


def empty():
    return {'external': False, 'local': []}


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    files = [ROOT / 'src/core.rs', ROOT / 'src/program.rs', ROOT / 'src/program_metadata.rs', ROOT / 'src/closure_dependencies.rs', ROOT / 'src/bin/closure_probe.rs', ROOT / 'corpus/dependency_policy.json', ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'scripts/core_export.exs', ROOT / 'scripts/closure_dependencies_oracle.exs', Path(__file__).resolve()]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in files}
    subprocess.run(['cargo', 'build', '--release', '--offline', '--bin', 'closure_probe'], cwd=ROOT, check=True)
    cases = json.loads((ROOT / '.cache/core/report.json').read_text())['cases']
    if len(cases) != 327:
        raise RuntimeError('closure corpus denominator changed')
    rows = []
    inputs = {}
    for i, case in enumerate(cases):
        source = Path(case['core_json'])
        inputs[str(source.relative_to(ROOT))] = digest(source)
        rust_path = OUT / f'{i}.rust.json'
        native_path = OUT / f'{i}.native.json'
        try:
            result = subprocess.run([str(ROOT / 'target/release/closure_probe'), str(source)], check=True, capture_output=True, timeout=60)
            rust_path.write_bytes(result.stdout)
            rust = json.loads(result.stdout)
            subprocess.run(['elixir', str(ROOT / 'scripts/closure_dependencies_oracle.exs'), str(source), str(rust_path), str(native_path)], cwd=ROOT, check=True, capture_output=True, timeout=90)
            native = json.loads(native_path.read_text())
            dependencies = [empty() for _ in rust['functions']]
            escaped, roots, external = empty(), empty(), empty()
            calls, letrecs = [], []
            for module in native:
                escaped = union(escaped, module['escaping'])
                calls.extend(module['calls'])
                letrecs.extend(module['letrecs'])
                for caller, targets in module['dependencies']:
                    if caller == 'top':
                        roots = union(roots, targets)
                    elif caller == 'external':
                        external = union(external, targets)
                    else:
                        dependencies[caller] = union(dependencies[caller], targets)
            expected = {'dependencies': dependencies, 'escaping': escaped, 'roots': roots, 'external_dependencies': external, 'calls': sorted(calls, key=lambda c: (c['module'], c['node'])), 'letrecs': sorted(letrecs, key=lambda c: c['binding'])}
            actual = {key: rust[key] for key in expected}
            actual['calls'] = sorted(actual['calls'], key=lambda c: (c['module'], c['node']))
            actual['letrecs'] = sorted(actual['letrecs'], key=lambda c: c['binding'])
            differences = {key: {'rust': actual[key], 'native': expected[key]} for key in expected if actual[key] != expected[key]}
            rows.append({'id': case['id'], 'passed': not differences, 'functions': len(rust['functions']), 'calls': len(calls), 'letrecs': len(letrecs), 'differences': differences})
        except (subprocess.SubprocessError, ValueError, KeyError) as error:
            rows.append({'id': case['id'], 'passed': False, 'error': str(error), 'stderr': getattr(error, 'stderr', b'').decode(errors='replace')})
        if (i + 1) % 25 == 0:
            print(f'{i + 1}/{len(cases)} checked; {sum(not r["passed"] for r in rows)} failures', flush=True)
    if any(digest(ROOT / name) != value for name, value in hashes.items()):
        raise RuntimeError('closure implementation changed during verification')
    if any(digest(ROOT / name) != value for name, value in inputs.items()):
        raise RuntimeError('closure inputs changed during verification')
    report = {'complete_closure_dependencies': all(r['passed'] for r in rows), 'implementation_sha256': hashes, 'inputs_sha256': inputs, 'cases': rows, 'requested': len(cases), 'passed': sum(r['passed'] for r in rows), 'functions': sum(r.get('functions', 0) for r in rows), 'calls': sum(r.get('calls', 0) for r in rows), 'letrecs': sum(r.get('letrecs', 0) for r in rows)}
    (OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: report[k] for k in ('passed', 'requested', 'functions', 'calls', 'letrecs')}))
    if not report['complete_closure_dependencies']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
