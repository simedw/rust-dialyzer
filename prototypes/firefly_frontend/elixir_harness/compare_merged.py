"""Alternate preserved split release binaries and the merged executable."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile

p = argparse.ArgumentParser()
p.add_argument('repo', type=Path)
p.add_argument('--native-bin', required=True, type=Path)
p.add_argument('--seed-bin', required=True, type=Path)
p.add_argument('--adapter-bin', required=True, type=Path)
p.add_argument('--probe', required=True, type=Path)
p.add_argument('--case', action='append', default=[])
p.add_argument('--rounds', type=int, default=5)
p.add_argument('--output', required=True, type=Path)
a = p.parse_args()
assert a.rounds > 0
base = Path(__file__).resolve().parent
cases = a.case or ['tiny', 'elixir/17_no_warning_on_exunit_assertions', 'dialyxir/examples']
case_args = [x for case in cases for x in ('--case', case)]
scripts = {'split': base.parent/'evidence/split-baseline-benchmark.py', 'merged': base/'benchmark.py'}
commands = {
    'split': [sys.executable, str(scripts['split']), str(a.repo.resolve()), '--seed-bin', str(a.seed_bin.resolve()),
              '--adapter-bin', str(a.adapter_bin.resolve()), '--probe', str(a.probe.resolve())],
    'merged': [sys.executable, str(scripts['merged']), str(a.repo.resolve()), '--native-bin', str(a.native_bin.resolve())],
}
samples = []
with tempfile.TemporaryDirectory(prefix='elixir-merged-comparison-') as temporary:
    for round_index in range(-1, a.rounds):
        order = ('split', 'merged') if round_index % 2 == 0 else ('merged', 'split')
        for name in order:
            output = Path(temporary)/f'{name}-{round_index}.json'
            subprocess.run(commands[name]+case_args+['--rounds', '1', '--warmups', '0', '--output', str(output)],
                           check=True, stdout=subprocess.DEVNULL)
            report = json.loads(output.read_text())
            assert report['warnings_verified_every_timed_run']
            if round_index >= 0:
                for sample in report['samples']:
                    sample['comparison_round'] = round_index
                    sample['implementation'] = name if sample['implementation'] == 'native' else 'otp'
                    samples.append(sample)
            print(f'{name} comparison round {round_index} complete', flush=True)
summary = []
for case in cases:
    timings = {name: statistics.median(s['wall_ms'] for s in samples if s['id'] == case and s['implementation'] == name)
               for name in ('split', 'merged', 'otp')}
    summary.append(dict(id=case, wall_ms=timings, merged_speedup_over_split=timings['split']/timings['merged'],
                        merged_speedup_over_otp=timings['otp']/timings['merged']))
report = dict(rounds=a.rounds, warmup_pairs=1, summary=summary, samples=samples,
              alternating_split_merged_order=True, fresh_processes=True, application_incremental_cache=False,
              warnings_verified_every_run=True, merged_signatures_verified_every_run=True,
              binary_sha256={str(path.resolve()):hashlib.sha256(path.read_bytes()).hexdigest()
                             for path in (a.native_bin,a.seed_bin,a.adapter_bin,a.probe)},
              scripts_sha256={str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in scripts.values()},
              limits=['OTP is timed alongside each split/merged sample, so each case has twice as many OTP observations.',
                      'Split baseline uses captured release binaries, including Rust1.92 analyzer and Rust1.88 frontend; merged uses Rust1.88 throughout.',
                      'Split includes native expected-warning comparison; merged comparisons run externally.',
                      'Frozen dependency facts and runtime profiles remain required.'])
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps(summary, indent=2))
