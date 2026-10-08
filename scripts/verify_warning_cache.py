#!/usr/bin/env python3
import hashlib
import itertools
import json
from pathlib import Path
import random
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.cache/warning_cache'
OUT.mkdir(parents=True, exist_ok=True)
files = [ROOT / name for name in ['src/incremental.rs', 'src/invalidation.rs', 'src/bin/warning_cache_probe.rs', 'scripts/warning_cache_oracle.exs', 'Cargo.toml', 'Cargo.lock']] + [Path(__file__).resolve()]
def hashes():
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
implementation = hashes()
subprocess.run(['cargo', 'build', '--release', '--offline', '--bin', 'warning_cache_probe'], cwd=ROOT, check=True)
rows = []
def emit(collected, unknown, stored, modules, old_dependencies=None, new_dependencies=None):
    rows.append(dict(id=len(rows), collected=collected, unknown=unknown, stored=stored, modules=modules, old_dependencies=old_dependencies or {}, new_dependencies=new_dependencies or {}))
def warnings(values):
    return [dict(module=m, warning=t) for m, t in values]
sequences = [[]] + [list(s) for n in (1, 2, 3) for s in itertools.product([('a','one'), ('b','two'), ('a','two')], repeat=n)]
for collected, unknown, stored, modules in itertools.product([None] + sequences, sequences[:4], [None, {}, {'a': ['old', 'old'], 'z': []}], [[], ['a'], ['b','a'], ['a','a'], ['absent']]):
    emit(None if collected is None else warnings(collected), warnings(unknown), stored, modules)
rng = random.Random(28306)
for _ in range(5000):
    def graph():
        return {m: [d for d in ['a','b','c','d'] if rng.randrange(2)] for m in ['a','b','c','d'] if rng.randrange(2)}
    emit([], [], {}, [], graph(), graph())
request = OUT / 'requests.jsonl'
request.write_text(''.join(json.dumps(r)+'\n' for r in rows))
for command, result in [(['elixir','scripts/warning_cache_oracle.exs'], 'native.jsonl'), ([str(ROOT/'target/release/warning_cache_probe')], 'rust.jsonl')]:
    with request.open() as inp, (OUT/result).open('w') as out:
        subprocess.run(command, cwd=ROOT, stdin=inp, stdout=out, check=True)
native = [json.loads(line) for line in (OUT/'native.jsonl').read_text().splitlines()]
rust = [json.loads(line) for line in (OUT/'rust.jsonl').read_text().splitlines()]
if len(rows) != 7460 or len(native) != len(rows) or len(rust) != len(rows):
    raise RuntimeError('warning cache denominator changed')
for row, a, b in zip(rows, native, rust):
    if not row['modules'] and a['selected'] is not None:
        a['selected'].sort()
        b['selected'].sort()
failed = [(a,b) for a,b in zip(native,rust) if a != b]
if failed:
    (OUT/'failures.json').write_text(json.dumps(failed[:20], indent=2))
    raise RuntimeError(f'{len(failed)} warning cache differences')
if implementation != hashes():
    raise RuntimeError('warning cache implementation changed during verification')
report = dict(commands=len(rows), matched=len(rows), complete_warning_storage_algebra=True, complete_disk_lifecycle=False, all_module_selection_order="membership only; native final diagnostic sorting remains separate", implementation_sha256=implementation, inputs_sha256={str(request.relative_to(ROOT)):hashlib.sha256(request.read_bytes()).hexdigest()})
(OUT/'report.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps({key:report[key] for key in ['commands','matched','complete_disk_lifecycle']}))
