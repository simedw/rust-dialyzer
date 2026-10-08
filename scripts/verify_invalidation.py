#!/usr/bin/env python3
"""Exercise native dependency expansion, removal and graph repair independently."""
import hashlib
import json
from pathlib import Path
import random
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.cache/invalidation'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    files = [ROOT / 'src/invalidation.rs', ROOT / 'src/bin/invalidation_probe.rs', ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'scripts/invalidation_oracle.exs', Path(__file__).resolve()]
    implementation = {str(p.relative_to(ROOT)): digest(p) for p in files}
    subprocess.run(['cargo', 'build', '--release', '--offline', '--bin', 'invalidation_probe'], cwd=ROOT, check=True)
    randomizer = random.Random(28319)
    commands = []
    for _ in range(5000):
        names = [f'm{i}' for i in range(randomizer.randrange(1, 33))]
        dependencies = {m: sorted(randomizer.sample(names, randomizer.randrange(len(names) + 1))) for m in names}
        changed = set(randomizer.sample(names, randomizer.randrange(len(names) + 1)))
        removed = set(randomizer.sample(names, randomizer.randrange(len(names) + 1))) - changed
        current = set(names) - removed
        commands.append(dict(changed=sorted(changed), removed=sorted(removed), dependencies=dependencies, current=sorted(current)))
    chain = [f'deep{i}' for i in range(10000)]
    commands.append(dict(changed=[chain[0]], removed=[], current=chain, dependencies={a: [b] for a, b in zip(chain, chain[1:])}))
    commands.append(dict(changed=[], removed=[chain[-1]], current=chain[:-1], dependencies={a: [b] for a, b in zip(chain, chain[1:])}))
    request_file = OUT / 'commands.jsonl'
    data = ''.join(json.dumps(c) + '\n' for c in commands)
    request_file.write_text(data)
    outputs = {}
    for name, command in [('rust', [str(ROOT / 'target/release/invalidation_probe')]), ('native', ['elixir', str(ROOT / 'scripts/invalidation_oracle.exs')])]:
        result = subprocess.run(command, input=data, text=True, capture_output=True, check=True, cwd=ROOT)
        (OUT / f'{name}.jsonl').write_text(result.stdout)
        outputs[name] = [json.loads(row) for row in result.stdout.splitlines()]
        if len(outputs[name]) != len(commands):
            raise RuntimeError(f'{name} result denominator changed')
    failures = [dict(request=commands[i], rust=rust, native=native) for i, (rust, native) in enumerate(zip(outputs['rust'], outputs['native'])) if rust != native]
    if implementation != {str(p.relative_to(ROOT)): digest(p) for p in files}:
        raise RuntimeError('invalidation implementation changed during verification')
    report = dict(complete_invalidation=not failures, complete_lifecycle=False, seed=28319, commands=len(commands), passed=len(commands)-len(failures), failures=failures, implementation_sha256=implementation, inputs_sha256={str(request_file.relative_to(ROOT)): digest(request_file)})
    (OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: report[k] for k in ('commands', 'passed', 'complete_invalidation')}))
    if failures:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
