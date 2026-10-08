#!/usr/bin/env python3
"""Run the unmodified upstream type test assertions through the Rust bridge."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.cache/type_suite'


def main():
    files = [ROOT / 'src/lattice.rs', ROOT / 'src/type_api.rs', ROOT / 'src/native_order.rs', ROOT / 'src/runtime_term.rs', ROOT / 'src/presentation.rs', ROOT / 'src/presentation_form.rs', ROOT / 'src/bin/type_probe.rs', ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'scripts/type_suite_bridge.exs', ROOT / 'scripts/bridge_native_types.exs', Path(__file__).resolve()]
    def hashes():
        return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
    implementation = hashes()
    subprocess.run(['cargo', 'build', '--release', '--offline', '--bin', 'type_probe'], cwd=ROOT, check=True)
    subprocess.run(['elixir', 'scripts/type_suite_bridge.exs', str(ROOT / '.cache/upstream/otp'), str(ROOT / 'target/release/type_probe'), str(OUT)], cwd=ROOT, check=True)
    if hashes() != implementation:
        raise RuntimeError('type suite implementation changed during verification')
    path = OUT / 'report.json'
    report = json.loads(path.read_text())
    required = {'consistency_and_to_string', 'misc', 'map_multiple_representations'}
    report['complete_original_type_tests'] = report['requested'] == report['passed'] == 3 and {c['case'] for c in report['cases']} == required and not report['bridge_unsupported'] and report['native_algebra_fallback'] is False
    report['implementation_sha256'] = implementation
    source = Path(report['original_suite'])
    report['inputs_sha256'] = {str(source.relative_to(ROOT)): hashlib.sha256(source.read_bytes()).hexdigest()}
    path.write_text(json.dumps(report, indent=2) + '\n')
    if not report['complete_original_type_tests']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
