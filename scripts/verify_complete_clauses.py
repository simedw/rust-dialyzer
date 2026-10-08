#!/usr/bin/env python3
"""Verify entire generated graphs, captures and refold allocation identities."""
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.cache/generation_complete'
OUT.mkdir(parents=True, exist_ok=True)
files = list((ROOT / 'src').rglob('*.rs')) + [ROOT / p for p in (
    'Cargo.toml', 'Cargo.lock', 'corpus/bifs.json',
    'scripts/generation_probes/native_complete_clauses.exs',
    'scripts/verify_complete_clauses.py', 'scripts/core_export.exs',
    'scripts/type_wire.exs', 'scripts/type_codec.exs',
)]
beam = Path(subprocess.check_output([
    'elixir', '-e', 'IO.puts(:code.which(:dialyzer_typesig))',
], cwd=ROOT, text=True).strip())
files.append(beam)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def hashes():
    return {str(path): sha(path) for path in files}


def norm(value):
    if isinstance(value, dict):
        if 'derived' in value:
            return {'derived': True}
        return {k: norm(v) for k, v in value.items() if k != 'closure_checks'}
    if isinstance(value, list):
        return [norm(v) for v in value]
    return value


before = hashes()
subprocess.run(['cargo', 'build', '--offline', '--release', '--bin', 'generation_probe'], cwd=ROOT, check=True)
binary = ROOT / 'target/release/generation_probe'
binary_hash = sha(binary)
commands = OUT / 'commands.jsonl'
subprocess.run([
    'elixir', str(ROOT / 'scripts/generation_probes/native_complete_clauses.exs'),
    str(ROOT), str(commands),
], cwd=ROOT, check=True)
input_hash = sha(commands)
rows = [json.loads(line) for line in commands.read_text().splitlines()]
assert len(rows) == 4520
actual_path = OUT / 'rust.jsonl'
with commands.open('rb') as source, actual_path.open('wb') as output:
    subprocess.run([str(binary)], cwd=ROOT, stdin=source, stdout=output, check=True)
actual = [json.loads(line) for line in actual_path.read_text().splitlines()]
assert len(actual) == len(rows)
failures = [{'index': i, 'native': n['native'], 'rust': a}
            for i, (n, a) in enumerate(zip(rows, actual))
            if norm(n['native']) != norm(a)]
(OUT / 'failures.json').write_text(json.dumps(failures[:20], indent=2) + '\n')
assert not failures, f'{len(failures)} graph or binding differences'
expected = sum(1 for row in rows for function in row['native']['functions']
               for node in function['graph']['nodes'] if node.get('right', {}).get('derived'))
closures = sum(a.get('closure_checks', 0) for a in actual)
assert closures == expected == 5612
allocations = sum(a['refold_allocations'] for a in actual)
assert allocations == 252
assert all(r['profile']['original_sha256'] == sha(beam).upper() for r in rows)
assert before == hashes()
assert sha(binary) == binary_hash and sha(commands) == input_hash
report = dict(cases=4520, matched=4520, closure_captures=closures,
              refold_allocations=allocations, full_core_generation=False,
              checked=['entire constraint graphs and dependencies', 'exact native closure captures',
                       'exported success bindings', 'fresh labels', 'relative synthetic identities'],
              remaining=['apply-target PLT disjunctions', 'imported signature and active contract fixtures',
                         'record-sensitive guard captures', 'production SCC orchestration'],
              implementation_sha256=before,
              inputs_sha256={str(binary): binary_hash, str(commands): input_hash})
(OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({k: report[k] for k in ('cases', 'matched', 'closure_captures', 'refold_allocations')}))
