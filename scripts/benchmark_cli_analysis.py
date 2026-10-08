#!/usr/bin/env python3
"""Compare full-analysis CLI latency with identical immutable BEAMs and PLT."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def warnings(stdout):
    lines = stdout.decode().splitlines()
    return '\n'.join(line for line in lines
                     if not line.startswith(('  Proceeding with analysis', ' done in ', 'done ('))).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust-root', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--rounds', type=int, default=3)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    rust_root = args.rust_root.resolve()
    binary = rust_root / 'target/release/rust-dialyzer-poc'
    native_root = Path('/Users/simedw/.asdf/installs/erlang/28.3')
    native = native_root / 'bin/dialyzer'
    elixir = Path('/Users/simedw/.asdf/installs/elixir/1.19.4-otp-28')
    prior = root / '.cache/corpus/otp/base.plt'
    out = args.out.resolve()
    assert not out.exists() and args.rounds > 0
    out.mkdir(parents=True)
    tiny = sorted((root / 'examples/tiny/_build/dev/lib/tiny/ebin').glob('*.beam'))
    assert len(tiny) == 4
    suite = Path('/private/tmp/rust-dialyzer-public-corpus-1/complete')
    originals = [json.loads(line) for line in (suite / 'public-native.jsonl').read_text().splitlines()]
    fixtures = [(row['id'], sorted((suite / 'inputs' / str(i)).glob('*.beam')))
                for i, row in enumerate(originals) if row['id'].startswith('elixir/')]
    assert len(fixtures) == 19 and all(files for _, files in fixtures)
    workloads = [('tiny', [('tiny', tiny)]), ('elixir_suite', fixtures)]
    paths = [Path(__file__), binary, native, prior, suite / 'public-native.jsonl']
    paths.extend(p for _, cases in workloads for _, files in cases for p in files)
    for directory in ('src', 'scripts', 'corpus'):
        paths.extend(p for p in (rust_root / directory).rglob('*') if p.is_file())
    for install in (native_root, elixir):
        paths.extend(p for p in (install / 'lib').rglob('*')
                     if p.is_file() and p.suffix in ('.beam', '.erl'))
    def inventory():
        return {str(p): sha(p) for p in sorted(set(paths))}
    before = inventory()
    env = os.environ.copy()
    env['PATH'] = str(native_root / 'bin') + os.pathsep + str(elixir / 'bin') + os.pathsep + env['PATH']
    result = []
    for workload, cases in workloads:
        samples = {'native': [], 'rust': []}
        for round_index in range(args.rounds + 1):
            totals = {'native': 0.0, 'rust': 0.0}
            for case_index, (case_id, files) in enumerate(cases):
                observed = {}
                for backend in (['native', 'rust'] if (round_index + case_index) % 2 == 0 else ['rust', 'native']):
                    command = ([str(native), '-pa', str(elixir / 'lib/elixir/ebin')]
                               if backend == 'native' else [str(binary)])
                    command += ['--no_check_plt', '--plt', str(prior), '-Wno_unknown', *map(str, files)]
                    start = time.perf_counter()
                    run = subprocess.run(command, cwd=root, env=env, capture_output=True, timeout=600)
                    elapsed = time.perf_counter() - start
                    assert run.returncode in (0, 2), (case_id, backend, run.returncode, run.stderr.decode()[-2000:])
                    filename = f'{workload}-{round_index}-{case_index}-{backend}'
                    (out / (filename + '.stdout')).write_bytes(run.stdout)
                    (out / (filename + '.stderr')).write_bytes(run.stderr)
                    observed[backend] = (run.returncode, warnings(run.stdout))
                    totals[backend] += elapsed
                    result.append(dict(workload=workload, case=case_id, round=round_index,
                                       warmup=round_index == 0, backend=backend, seconds=elapsed,
                                       exit_status=run.returncode, command=command))
                assert observed['native'] == observed['rust'], (case_id, 'warning output mismatch', observed)
            if round_index:
                for backend, elapsed in totals.items():
                    samples[backend].append(elapsed)
            print(json.dumps(dict(workload=workload, round=round_index, seconds=totals)), flush=True)
        medians = {backend: statistics.median(values) for backend, values in samples.items()}
        summary = dict(workload=workload, groups=len(cases), modules=sum(len(files) for _, files in cases),
                       samples=samples, median_seconds=medians,
                       rust_over_native=medians['rust'] / medians['native'])
        (out / (workload + '.json')).write_text(json.dumps(summary, indent=2) + '\n')
    assert inventory() == before, 'source, binary, runtime, BEAM or PLT changed during benchmark'
    (out / 'runs.json').write_text(json.dumps(result, indent=2) + '\n')
    (out / 'provenance.json').write_text(json.dumps(before, indent=2) + '\n')
    report = dict(results=[json.loads((out / (name + '.json')).read_text()) for name, _ in workloads],
                  warnings_exact=True, inputs_unchanged=True, rounds=args.rounds,
                  scope='Full analysis with an existing dependency PLT; CLI startup, compiler export, inference and rendering included; no incremental result cache; PLT construction and Mix compilation excluded.')
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report), flush=True)


if __name__ == '__main__':
    main()
