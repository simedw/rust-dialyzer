"""Correctness-checked fresh-process timing with an existing immutable dependency PLT."""
import argparse
from functools import lru_cache
import hashlib
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import tempfile
import time
from native_requests import request

parser = argparse.ArgumentParser()
parser.add_argument('repo', type=Path)
parser.add_argument('--native-bin', required=True, type=Path)
parser.add_argument('--case', action='append', default=[])
parser.add_argument('--rounds', type=int, default=5)
parser.add_argument('--warmups', type=int, default=1)
parser.add_argument('--elixir', default='elixir')
parser.add_argument('--output', type=Path)
parser.add_argument('--phase-timings', action='store_true', help='Enable opt-in Rust stderr phase timers and verify release build modes')
args = parser.parse_args()
assert args.rounds > 0 and args.warmups >= 0
repo = args.repo.resolve()
base = Path(__file__).resolve().parent
oracle = base/'generated/oracle'
rows = [json.loads(line) for line in (oracle/'oracle-rows.jsonl').read_text().splitlines()]
chosen = args.case or ['tiny']
rows = [r for r in rows if r['id'] in chosen]
assert set(chosen) == {r['id'] for r in rows}, 'unknown case'
# A speed result is allowed only after the exact selected semantic gate passed.
gate = json.loads((base/'generated/native/report.json').read_text())
passed = {c['id'] for c in gate['checks'] if c['warnings_match'] and c['inferred_types_match']}
assert set(chosen) <= passed, 'selected native cases have not passed correctness gate'
assert not gate['native_path_launched_vm']
native_bin = args.native_bin.resolve()
assert gate['sha256'][str(native_bin)] == hashlib.sha256(native_bin.read_bytes()).hexdigest(), 'native binary changed since gate'
prior_path = oracle/'prior-library.json'
prior = json.loads(prior_path.read_text())
plt = Path(prior['source']['path'])
plt_sha = hashlib.sha256(plt.read_bytes()).hexdigest()
assert plt_sha == prior['source']['sha256'].lower(), 'frozen prior snapshot differs from PLT'
provenance = json.loads((oracle/'capture-provenance.json').read_text())
assert plt.resolve() == Path(provenance['prior_path']).resolve()
selected = json.loads((repo/'.cache/corpus/elixir-selected.json').read_text())['cases']
warning_options = {c['id']: c['warning_options'] for c in selected}
warning_options['tiny'] = []
baseline = {r['id']:r for r in map(json.loads,(oracle/'baseline-results.jsonl').read_text().splitlines())}
elixir = shutil.which(args.elixir)
assert elixir, 'Elixir executable unavailable'


def expand(arena):
    terms = arena['terms']
    @lru_cache(None)
    def at(i):
        t = terms[i]
        k = t['kind']
        if k == 'nil':
            return {'kind':'list','items':[],'tail':None}
        if k == 'tuple':
            return {'kind':k,'items':[at(x) for x in t['items']]}
        if k == 'list':
            items = [at(x) for x in t['items']]
            tail = at(t['tail'])
            if tail['kind']=='list':
                items += tail['items']
                tail = tail['tail']
            return {'kind':k,'items':items,'tail':tail}
        return t
    return at(arena['root'])


def run(command, **kwargs):
    start = time.perf_counter_ns()
    process = subprocess.run([str(x) for x in command], capture_output=True, text=True, check=True, **kwargs)
    return process, (time.perf_counter_ns()-start)/1e6


def phase_records(process, module=None):
    records = [json.loads(line.removeprefix('RUST_DIALYZER_TIMING '))
               for line in process.stderr.splitlines() if line.startswith('RUST_DIALYZER_TIMING ')]
    if args.phase_timings:
        assert records, 'instrumentation missing from Rust executable'
        assert all(r['build_mode'] == 'release' for r in records), 'instrumented benchmark requires release Rust builds'
    if module:
        records = [dict(record, module=module) for record in records]
    return records


samples = []
with tempfile.TemporaryDirectory(prefix='elixir-native-benchmark-') as scratch:
    scratch = Path(scratch)
    trap = scratch/'vm-trap'
    trap.mkdir()
    trap_log = scratch/'vm.log'
    for name in ('erl','elixir','escript','mix'):
        file = trap/name
        file.write_text('#!/bin/sh\nprintf "%s\\n" "$0" >> "$NATIVE_VM_TRAP_LOG"\nexit 97\n')
        file.chmod(0o755)
    native_env = dict(os.environ,PATH=str(trap),NATIVE_VM_TRAP_LOG=str(trap_log))
    native_env.pop('RUST_DIALYZER_TIMINGS', None)
    if args.phase_timings:
        native_env['RUST_DIALYZER_TIMINGS'] = '1'
    for row in rows:
        for beam,digest in row['beam_sha256'].items():
            assert hashlib.sha256(Path(beam).read_bytes()).hexdigest() == digest
        config = scratch/'otp-config.json'
        config.write_text(json.dumps(dict(beams=row['beam_paths'],plt=str(plt),warning_options=warning_options[row['id']])))
        expected_warnings = expand(row['warnings'])
        for round_index in range(-args.warmups,args.rounds):
            # Alternate order to reduce systematic ordering bias.
            ordering = ('otp','native') if round_index % 2 == 0 else ('native','otp')
            for implementation in ordering:
                if implementation == 'otp':
                    process,wall_ms = run([elixir,base/'benchmark.exs',config])
                    value = json.loads(process.stdout.splitlines()[-1])
                    # Equality checks occur after the timers.
                    assert all(value['profile'][k] == row['core']['profile'][k] for k in ('otp','erts','elixir')), 'OTP runtime profile changed'
                    assert value['warning_count']==row['warning_count']
                    assert value['warnings']==expected_warnings, f"OTP warnings changed: {row['id']}"
                    sample = dict(id=row['id'],implementation='otp',round=round_index,wall_ms=wall_ms,
                                  analysis_ms=value['dialyzer_ns']/1e6,profile=value['profile'])
                else:
                    native_input = json.dumps(request(row))+'\n'
                    process, wall_ms = run([native_bin, prior_path, oracle/'closure-profile.json'],
                                           env=native_env, input=native_input)
                    value = json.loads(process.stdout)
                    timings = phase_records(process)
                    assert 'error' not in value, value
                    assert value['public_warning_count'] == row['warning_count']
                    assert expand(value['warnings']) == expected_warnings, f"native warnings changed: {row['id']}"
                    signatures = lambda value: sorted(value.get('signatures', []), key=lambda v:(v['module'],v['function'],v['arity']))
                    assert signatures(value) == signatures(baseline[row['id']]), f"native signatures changed: {row['id']}"
                    assert 'core' not in value, 'timed run emitted verification-only Core'
                    sample = dict(id=row['id'], implementation='native', round=round_index, wall_ms=wall_ms)
                    if args.phase_timings:
                        sample['rust_phase_timings'] = timings
                assert not trap_log.exists(), 'native pipeline attempted VM launch'
                if round_index >= 0:
                    samples.append(sample)
                print(f"{row['id']} {implementation} round {round_index}: {sample['wall_ms']:.2f} ms",flush=True)
assert hashlib.sha256(plt.read_bytes()).hexdigest()==plt_sha, 'benchmark mutated dependency PLT'
summary = []
for row in rows:
    case = dict(id=row['id'],modules=len(row['beam_paths']),warnings=row['warning_count'])
    for implementation in ('otp','native'):
        measurements = [s for s in samples if s['id']==row['id'] and s['implementation']==implementation]
        keys = [k for k in measurements[0] if k.endswith('_ms')]
        case[implementation] = {key:statistics.median(s[key] for s in measurements) for key in keys}
    case['native_speedup'] = case['otp']['wall_ms']/case['native']['wall_ms']
    if args.phase_timings:
        native = [s for s in samples if s['id']==row['id'] and s['implementation']=='native']
        scopes = sorted({r['scope'] for s in native for r in s['rust_phase_timings']})
        totals = []
        for scope in scopes:
            rounds = [[r for r in s['rust_phase_timings'] if r['scope']==scope] for s in native]
            names = sorted({p['name'] for records in rounds for r in records for p in r['phases']})
            totals.append(dict(scope=scope, total_ms=statistics.median(sum(r['total_ms'] for r in records) for records in rounds),
                               phases={name:statistics.median(sum(p['ms'] for r in records for p in r['phases'] if p['name']==name)
                                                               for records in rounds) for name in names}))
        case['rust_phase_timings'] = totals
    summary.append(case)
report = dict(rounds=args.rounds,warmups=args.warmups,summary=summary,samples=samples,
              rust_phase_timings_enabled=args.phase_timings,
              rust_build_modes_verified=['release'] if args.phase_timings else [],
              warnings_verified_every_timed_run=True,inferred_types_verified_every_native_run=True,native_process_count=1,intermediate_frontend_files=False,application_incremental_cache=False,warm_filesystem=True,
              startup_included=True,captured_runtime_profiles=True,optimized_binaries_expected=True,dependency_plt=str(plt),dependency_plt_sha256=plt_sha,
              limits=['Native pipeline runs ingestion, lowering and analysis in one process without intermediate frontend files.',
                      'Native context-and-analysis includes loading captured JSON dependency facts; OTP loads its existing binary PLT.',
                      'No application rebuild or incremental application cache is timed.',
                      'OTP subprocess wall includes the Elixir script runner, runtime startup, and warning serialization after its internal analysis timer.',
                      'Native result projection is timed; expected-warning and signature comparisons occur outside the native subprocess.'],
              binary_sha256={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (native_bin,)},
              script_sha256={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (Path(__file__),base/'native_requests.py',base/'benchmark.exs')})
output = args.output or base/'generated/benchmark.json'
output.parent.mkdir(parents=True,exist_ok=True)
output.write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(summary,indent=2))
