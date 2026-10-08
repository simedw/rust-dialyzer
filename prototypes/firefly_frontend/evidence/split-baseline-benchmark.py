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

parser = argparse.ArgumentParser()
parser.add_argument('repo', type=Path)
parser.add_argument('--seed-bin', required=True, type=Path)
parser.add_argument('--adapter-bin', required=True, type=Path)
parser.add_argument('--probe', required=True, type=Path)
parser.add_argument('--case', action='append', default=[])
parser.add_argument('--rounds', type=int, default=5)
parser.add_argument('--warmups', type=int, default=1)
parser.add_argument('--elixir', default='elixir')
parser.add_argument('--output', type=Path)
parser.add_argument('--phase-timings', action='store_true', help='Enable opt-in Rust stderr phase timers and verify release build modes')
args = parser.parse_args()
assert args.rounds > 0 and args.warmups >= 0
repo = args.repo.resolve()
base = repo/'prototypes/firefly_frontend/elixir_harness'
oracle = base/'generated/oracle'
rows = [json.loads(line) for line in (oracle/'oracle-rows.jsonl').read_text().splitlines()]
chosen = args.case or ['tiny']
rows = [r for r in rows if r['id'] in chosen]
assert set(chosen) == {r['id'] for r in rows}, 'unknown case'
# A speed result is allowed only after the exact selected semantic gate passed.
gate = json.loads((base.parent/'evidence/elixir-instrumented-gate.json').read_text())
passed = {c['id'] for c in gate['checks'] if c['warnings_match'] and c['inferred_types_match']}
assert set(chosen) <= passed, 'selected native cases have not passed correctness gate'
assert not gate['native_path_launched_vm']
seed_bin, adapter_bin, probe = (p.resolve() for p in (args.seed_bin,args.adapter_bin,args.probe))
assert gate['sha256'][str(seed_bin)] == hashlib.sha256(seed_bin.read_bytes()).hexdigest(), 'seed binary changed since gate'
assert gate['sha256'][str(adapter_bin)] == hashlib.sha256(adapter_bin.read_bytes()).hexdigest(), 'adapter binary changed since gate'
assert gate['sha256'][str(probe)] == hashlib.sha256(probe.read_bytes()).hexdigest(), 'analyzer binary changed since gate'
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
                    started = time.perf_counter_ns()
                    translation_ms,adapter_ms = 0.0,0.0
                    modules = []
                    timings = []
                    for beam in row['beam_paths']:
                        name = Path(beam).stem
                        seed,module = scratch/(name+'.seed.json'),scratch/(name+'.module.json')
                        process,elapsed = run([seed_bin,beam,seed],env=native_env)
                        timings.extend(phase_records(process, name))
                        translation_ms += elapsed
                        process,elapsed = run([adapter_bin,beam,seed,module],env=native_env)
                        timings.extend(phase_records(process, name))
                        adapter_ms += elapsed
                        modules.append(json.loads(module.read_text()))
                    # Only runtime profiles are imported; executable nodes and metadata are native-produced.
                    core = {k:row['core'][k] for k in ('version','profile','atom_layout','contract_closure_profile')}
                    core['modules'] = sorted(modules,key=lambda m:m['name'])
                    native_row = dict(row,core=core)
                    process,analysis_ms = run([probe,prior_path,oracle/'closure-profile.json'],
                                             env=native_env,input=json.dumps(native_row)+'\n')
                    wall_ms = (time.perf_counter_ns()-started)/1e6
                    value = json.loads(process.stdout)
                    timings.extend(phase_records(process))
                    assert value['public_warnings_match'] is True, f"native warnings changed: {row['id']}"
                    assert value['public_warning_count']==row['warning_count']
                    assert expand(value['warnings'])==expected_warnings
                    sample = dict(id=row['id'],implementation='native',round=round_index,wall_ms=wall_ms,
                                  translation_ms=translation_ms,adapter_ms=adapter_ms,context_and_analysis_ms=analysis_ms)
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
              warnings_verified_every_timed_run=True,application_incremental_cache=False,warm_filesystem=True,
              startup_included=True,captured_runtime_profiles=True,optimized_binaries_expected=True,dependency_plt=str(plt),dependency_plt_sha256=plt_sha,
              limits=['Prototype native pipeline uses separate helper and adapter processes per module and flat JSON transport.',
                      'Native context-and-analysis includes loading captured JSON dependency facts; OTP loads its existing binary PLT.',
                      'No application rebuild or incremental application cache is timed.',
                      'OTP subprocess wall includes the Elixir script runner, runtime startup, and warning serialization after its internal analysis timer.',
                      'Native analyzer subprocess includes correctness comparison against expected warnings.'],
              binary_sha256={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (seed_bin,adapter_bin,probe)},
              script_sha256={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (Path(__file__),base/'benchmark.exs')})
output = args.output or base/'generated/benchmark.json'
output.parent.mkdir(parents=True,exist_ok=True)
output.write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(summary,indent=2))
