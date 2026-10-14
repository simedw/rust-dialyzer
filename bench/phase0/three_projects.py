#!/usr/bin/env python3
"""Freeze BEAM inputs and compare cold native Rust with a joint OTP analysis.

prepare CONFIG OUT copies the inputs listed by CONFIG's project entries.
run OUT BINARY measures alternating fresh processes and verifies raw warnings.
Every run uses an empty prior, with no Core cache or incremental reuse.
"""
import argparse
import collections
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import sys
import threading
import time

from compare_warnings import expand, items
from compare_signatures import index_result
from storage import require_disk_reserve

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, data):
    path.write_text(json.dumps(data, indent=2) + '\n')


def prepare(config, out):
    require_disk_reserve(out)
    out.mkdir(parents=True, exist_ok=False)
    for name, entry in config['projects'].items():
        directory = out / name
        directory.mkdir()
        source = json.loads(Path(entry['manifest']).read_text())
        options = json.loads(Path(entry['options']).read_text())
        provenance = []
        paths = []
        for original in source['beam_files']:
            data = Path(original).read_bytes()
            sha = hashlib.sha256(data).hexdigest()
            frozen = out / 'beams' / sha / Path(original).name
            frozen.parent.mkdir(parents=True, exist_ok=True)
            if not frozen.exists():
                frozen.write_bytes(data)
            paths.append(str(frozen.resolve()))
            provenance.append({'source': original, 'frozen': str(frozen.resolve()), 'sha256': sha})
        source['beam_files'] = paths
        for key in ['dialyzer_typesig_beam', 'dialyzer_contracts_beam']:
            original = Path(source[key])
            frozen = out / original.name
            frozen.write_bytes(original.read_bytes())
            source[key] = str(frozen.resolve())
        write(directory / 'manifest.json', source)
        write(directory / 'options.json', options)
        write(directory / 'inputs.json', provenance)
        (directory / 'beams.txt').write_text('\n'.join(paths) + '\n')
        print(name, len(paths), 'frozen modules', flush=True)
    write(out / 'config.json', config)


def warning_set(result):
    raw = result['warnings']
    tree = expand(raw) if 'terms' in raw else raw
    return collections.Counter(json.dumps(row, sort_keys=True) for row in items(tree))


def check_same(reference, candidate):
    def read(path):
        return index_result(path if path.exists() else Path(str(path) + '.gz'))
    before_signatures, before = read(reference)
    after_signatures, after = read(candidate)
    assert before['status'] == after['status'] == 'ok'
    assert before['modules'] == after['modules']
    assert warning_set(before) == warning_set(after), 'raw warnings differ'
    assert before_signatures == after_signatures, 'inferred signatures differ'
    return {'modules': after['modules'], 'warnings': after['public_warning_count'],
            'signatures': len(after_signatures), 'exact_warning_and_signature_parity': True,
            'load_s': after['load_seconds'], 'analysis_s': after['analysis_seconds']}


def host_pressure():
    result = {}
    for key, command in [('swap', ['/usr/sbin/sysctl', '-n', 'vm.swapusage']),
                         ('memory_pressure', ['/usr/bin/memory_pressure', '-Q'])]:
        try:
            observation = subprocess.run(command, capture_output=True, text=True, timeout=2)
            result[key] = observation.stdout.strip() if observation.returncode == 0 else None
        except (OSError, subprocess.TimeoutExpired):
            result[key] = None
    return result


def measure(command, directory, label, env):
    waiting, deadline = [], time.monotonic() + 300
    while True:
        try:
            require_disk_reserve(directory)
            break
        except RuntimeError:
            waiting.append({'epoch_s': time.time(), 'load': os.getloadavg(),
                            'disk_free_bytes': shutil.disk_usage(directory).free, **host_pressure()})
            write(directory / (label + '-reserve-wait.json'), waiting)
            if time.monotonic() >= deadline:
                raise
            print(label, 'waiting for disk reserve', flush=True)
            time.sleep(10)
    before = os.getloadavg()
    samples, stopped = [], threading.Event()
    started = time.monotonic()
    def sample_load():
        while not stopped.is_set():
            sample = {'elapsed_s': time.monotonic() - started, 'load': os.getloadavg(),
                      'disk_free_bytes': shutil.disk_usage(directory).free, **host_pressure()}
            samples.append(sample)
            stopped.wait(10)
    sampler = threading.Thread(target=sample_load, daemon=True)
    sampler.start()
    try:
        with (directory / (label + '.stdout')).open('w') as stdout, (directory / (label + '.stderr')).open('w') as stderr:
            result = subprocess.run(['/usr/bin/time', '-l', *map(str, command)], cwd=ROOT,
                                    env=env, stdout=stdout, stderr=stderr)
    finally:
        stopped.set()
        sampler.join()
        write(directory / (label + '-load.json'), samples)
    if result.returncode:
        raise RuntimeError(f'{label} exited {result.returncode}: {(directory / (label + ".stderr")).read_text()[-3000:]}')
    text = (directory / (label + '.stderr')).read_text()
    def number(pattern):
        return float(re.search(pattern, text).group(1))
    metrics = {'wall_s': number(r'([\d.]+) real'), 'user_s': number(r'([\d.]+) user'),
            'sys_s': number(r'([\d.]+) sys'), 'peak_bytes': number(r'(\d+)\s+peak memory footprint'),
            'max_rss_bytes': number(r'(\d+)\s+maximum resident set size'),
            'instructions': number(r'(\d+)\s+instructions retired'),
            'load_before': before, 'load_after': os.getloadavg(), 'load_samples': samples,
            'stdout_sha256': digest(directory / (label + '.stdout'))}
    write(directory / (label + '-metrics.json'), metrics)
    return metrics


def run(out, binary, repetitions, projects, label, timings, warnings_only, resume=False):
    with open('/private/tmp/rust-dialyzer-run.lock', 'a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        config = json.loads((out / 'config.json').read_text())
        binary = binary.resolve()
        destination = out / label
        destination.mkdir(exist_ok=resume)
        copied = destination / 'core_cache_analyze'
        if resume:
            metadata = json.loads((destination / 'metadata.json').read_text())
            assert digest(copied) == metadata['binary_sha256'] == digest(binary), 'resume binary differs'
            assert metadata['timings_enabled'] == timings, 'resume timing mode differs'
            assert metadata['signature_export'] == (not warnings_only), 'resume output mode differs'
        else:
            copied.write_bytes(binary.read_bytes())
            copied.chmod(0o555)
        env = dict(os.environ, ERL_FLAGS='+S 14:14')
        for key in ['RUST_DIALYZER_TIMINGS', 'RUST_DIALYZER_VERIFY_FASTPATH', 'RUST_DIALYZER_SEQUENTIAL', 'RUST_DIALYZER_JOB_TRACE', 'RUST_DIALYZER_JOBTRACE', 'RUST_DIALYZER_GENERATION_PROBE']:
            env.pop(key, None)
        if timings:
            env['RUST_DIALYZER_TIMINGS'] = '1'
        if not resume:
            metadata = {
                'binary_sha256': digest(copied),
                'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                'source_sha256': {str(p.relative_to(ROOT)): digest(p) for p in [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', *sorted((ROOT / 'src').rglob('*.rs'))]},
                'repetitions': repetitions, 'timings_enabled': timings, 'signature_export': not warnings_only,
                'scope': 'Identical frozen BEAMs; cold native frontend; empty Rust prior; fresh OTP PLT. With report_modules, OTP builds dependency facts then checks application modules; otherwise one joint OTP analysis reports all warnings. 14 schedulers; filesystem caches are not flushed.',
            }
        contract = {'projects': list(projects or config['projects']),
                    'prior_sha256': digest(Path(config['prior'])),
                    'options_sha256': {name: digest(out / name / 'options.json') for name in projects or config['projects']},
                    'manifest_sha256': {name: digest(out / name / 'manifest.json') for name in projects or config['projects']},
                    'inventory_sha256': {name: digest(out / name / 'inputs.json') for name in projects or config['projects']},
                    'oracle_sha256': digest(ROOT / 'bench/phase0/cold_oracle.exs'),
                    'environment': {k: v for k, v in env.items() if k.startswith(('RUST_DIALYZER_', 'MIMALLOC_')) or k == 'ERL_FLAGS'}}
        if resume:
            assert metadata.get('resume_contract') == contract, 'resume inputs/oracle/environment differ'
            assert repetitions >= metadata['repetitions'], 'resume cannot reduce the measured sample count'
            metadata['repetitions'] = repetitions
            metadata.setdefault('resumes', []).append({'epoch_s': time.time(), 'harness_sha256': digest(Path(__file__))})
        else:
            metadata['resume_contract'] = contract
        write(destination / 'metadata.json', metadata)
        saved = destination / 'measurements.json'
        rows = json.loads(saved.read_text()) if resume and saved.exists() else []
        for name in projects or config['projects']:
            inventory = out / name
            for entry in json.loads((inventory / 'inputs.json').read_text()):
                assert digest(entry['frozen']) == entry['sha256'], entry['frozen']
            for repeat in range(repetitions + 1):
                if any(row['project'] == name and row['repeat'] == repeat for row in rows):
                    continue
                directory = destination / name / str(repeat)
                directory.mkdir(parents=True, exist_ok=resume)
                commands = {
                    'rust': [copied, config['prior'], inventory / 'manifest.json', inventory / 'options.json',
                             *(['--warnings-only'] if warnings_only else [])],
                    'otp': [config['elixir'], ROOT / 'bench/phase0/cold_oracle.exs', ROOT, inventory, directory],
                }
                measurements = {}
                for engine in (['rust', 'otp'] if repeat % 2 == 0 else ['otp', 'rust']):
                    metrics_path = directory / (engine + '-metrics.json')
                    output_path = directory / (engine + '.stdout')
                    if resume and metrics_path.exists() and output_path.exists() and (engine != 'otp' or (directory / 'otp-warnings.json').exists()):
                        prior = json.loads(metrics_path.read_text())
                        assert prior['stdout_sha256'] == digest(output_path), 'resumed output changed'
                        measurements[engine] = prior
                        print(name, repeat, engine, 'reuse completed measurement', flush=True)
                        continue
                    measurements[engine] = measure(commands[engine], directory, engine, env)
                    print(name, repeat, engine, measurements[engine]['wall_s'], 's', flush=True)
                rust = json.loads((directory / 'rust.stdout').read_text())
                otp = json.loads((directory / 'otp-warnings.json').read_text())
                assert rust['status'] == 'ok', rust
                assert warning_set(rust) == warning_set(otp), f'{name}: raw warnings differ'
                assert rust['modules'] == len(json.loads((inventory / 'inputs.json').read_text()))
                row = {'project': name, 'repeat': repeat, 'warmup': repeat == 0, 'rust': measurements['rust'], 'otp': measurements['otp'],
                       'modules': rust['modules'], 'warnings': rust['public_warning_count'],
                       'signatures': len(rust['signatures']) if not warnings_only else None,
                       'load_s': rust['load_seconds'], 'analysis_s': rust['analysis_seconds'], 'raw_warning_parity': True}
                rows.append(row)
                write(destination / 'measurements.json', rows)
                # The timing files and warning oracle are small; keep one complete Rust output for signature comparisons.
                if repeat > 1:
                    (directory / 'rust.stdout').unlink()
                (directory / 'otp.plt').unlink(missing_ok=True)
            summary = {}
            for project in set(r['project'] for r in rows):
                selected = [r for r in rows if r['project'] == project and not r['warmup']]
                rust_s = statistics.median(r['rust']['wall_s'] for r in selected)
                otp_s = statistics.median(r['otp']['wall_s'] for r in selected)
                summary[project] = {'rust_s': rust_s, 'otp_s': otp_s, 'speedup': otp_s / rust_s,
                                    'at_least_3x': otp_s >= 3 * rust_s, 'raw_warning_parity': True,
                                    'modules': selected[0]['modules'], 'pairs': len(selected)}
            write(destination / 'summary.json', summary)
            print(json.dumps(summary), flush=True)


def parse_env_flags(flags):
    result = {}
    for flag in flags or []:
        key, sep, value = flag.partition('=')
        if not sep or not key.startswith(('RUST_DIALYZER_', 'MIMALLOC_')):
            raise ValueError('benchmark environment flags must be RUST_DIALYZER_* or MIMALLOC_* KEY=VALUE')
        result[key] = value
    return result


def ab(out, baseline, candidate, repetitions, projects, label, baseline_env=None, candidate_env=None, resume=False):
    with open('/private/tmp/rust-dialyzer-run.lock', 'a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        config = json.loads((out / 'config.json').read_text())
        destination = out / label
        destination.mkdir(exist_ok=resume)
        env = dict(os.environ)
        for key in ['RUST_DIALYZER_TIMINGS', 'RUST_DIALYZER_VERIFY_FASTPATH', 'RUST_DIALYZER_SEQUENTIAL', 'RUST_DIALYZER_JOB_TRACE', 'RUST_DIALYZER_JOBTRACE', 'RUST_DIALYZER_GENERATION_PROBE']:
            env.pop(key, None)
        environments = {variant: dict(env, **parse_env_flags(flags)) for variant, flags in
                        [('baseline', baseline_env), ('candidate', candidate_env)]}
        binaries = {}
        metadata = json.loads((destination / 'metadata.json').read_text()) if resume else None
        overrides = {'baseline': parse_env_flags(baseline_env), 'candidate': parse_env_flags(candidate_env)}
        if resume:
            assert metadata['environment_overrides'] == overrides, 'resume environment differs'
        for name, original in [('baseline', baseline), ('candidate', candidate)]:
            binary = destination / name
            if resume:
                assert digest(binary) == metadata['binary_sha256'][name] == digest(original), 'resume binary differs'
            else:
                binary.write_bytes(original.read_bytes())
                binary.chmod(0o555)
            binaries[name] = binary
        if resume:
            metadata.setdefault('resumes', []).append({'epoch_s': time.time(), 'harness_sha256': digest(Path(__file__))})
        else:
            metadata = {'binary_sha256': {k: digest(v) for k, v in binaries.items()},
            'environment_overrides': overrides,
            'metric': 'Native BEAM loading and complete analysis; same frozen BEAMs and empty prior; timing excludes parity comparison',
            'source_sha256': {str(p.relative_to(ROOT)): digest(p) for p in [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', *sorted((ROOT / 'src').rglob('*.rs'))]}}
        write(destination / 'metadata.json', metadata)
        saved = destination / 'measurements.json'
        rows = json.loads(saved.read_text()) if resume and saved.exists() else []
        for project in projects or config['projects']:
            inventory = out / project
            for entry in json.loads((inventory / 'inputs.json').read_text()):
                assert digest(entry['frozen']) == entry['sha256'], entry['frozen']
            for repeat in range(repetitions + 1):
                if any(row['project'] == project and row['repeat'] == repeat for row in rows):
                    continue
                directory = destination / project / str(repeat)
                directory.mkdir(parents=True, exist_ok=True)
                measurements = {}
                for variant in (['baseline', 'candidate'] if repeat % 2 == 0 else ['candidate', 'baseline']):
                    metrics_path = directory / (variant + '-metrics.json')
                    output_path = directory / (variant + '.stdout')
                    if resume and metrics_path.exists() and output_path.exists():
                        prior = json.loads(metrics_path.read_text())
                        checksum = digest(output_path)
                        if 'stdout_sha256' in prior:
                            assert prior['stdout_sha256'] == checksum, 'resumed output changed'
                        else:
                            prior['stdout_sha256'] = checksum
                            prior['checksum_captured_on_resume'] = True
                            write(metrics_path, prior)
                        measurements[variant] = prior
                        print(project, repeat, variant, 'reuse completed measurement', flush=True)
                        continue
                    for suffix in ['stdout', 'stderr', '-load.json', '-reserve-wait.json']:
                        path = directory / (variant + (suffix if suffix.startswith('-') else '.' + suffix))
                        if path.exists():
                            path.rename(path.with_name(path.name + f'.interrupted-{time.time_ns()}'))
                    measurements[variant] = measure([binaries[variant], config['prior'], inventory / 'manifest.json', inventory / 'options.json'], directory, variant, environments[variant])
                    print(project, repeat, variant, measurements[variant]['wall_s'], flush=True)
                proof = check_same(directory / 'baseline.stdout', directory / 'candidate.stdout')
                row = {'project': project, 'repeat': repeat, 'warmup': repeat == 0, **measurements, **proof}
                rows.append(row)
                write(destination / 'measurements.json', rows)
                if repeat > 0:
                    for variant in binaries:
                        (directory / (variant + '.stdout')).unlink()
            selected = [r for r in rows if r['project'] == project and not r['warmup']]
            summary = {key: statistics.median(r[key]['wall_s'] for r in selected) for key in binaries}
            summary['speedup'] = summary['baseline'] / summary['candidate']
            print(project, json.dumps(summary), flush=True)
            write(destination / (project + '-summary.json'), summary)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    prep = sub.add_parser('prepare')
    prep.add_argument('config', type=Path)
    prep.add_argument('out', type=Path)
    bench = sub.add_parser('run')
    bench.add_argument('out', type=Path)
    bench.add_argument('binary', type=Path)
    bench.add_argument('--repetitions', type=int, default=3)
    bench.add_argument('--project', action='append')
    bench.add_argument('--label', required=True)
    bench.add_argument('--timings', action='store_true')
    bench.add_argument('--warnings-only', action='store_true', help='Match the CLI warning projection; verify full signatures separately')
    bench.add_argument('--resume', action='store_true', help='Resume unchanged inputs/binary/environment; keep completed pairs and engines')
    compare = sub.add_parser('ab')
    compare.add_argument('out', type=Path)
    compare.add_argument('baseline', type=Path)
    compare.add_argument('candidate', type=Path)
    compare.add_argument('--repetitions', type=int, default=3)
    compare.add_argument('--project', action='append')
    compare.add_argument('--label', required=True)
    compare.add_argument('--resume', action='store_true', help='Resume with the same frozen binaries and environment; rerun incomplete pairs')
    compare.add_argument('--baseline-env', action='append', help='RUST_DIALYZER_* or MIMALLOC_* KEY=VALUE')
    compare.add_argument('--candidate-env', action='append', help='RUST_DIALYZER_* or MIMALLOC_* KEY=VALUE')
    args = parser.parse_args()
    if args.command == 'prepare':
        prepare(json.loads(args.config.read_text()), args.out.resolve())
    elif args.command == 'run':
        if args.repetitions < 1:
            parser.error('repetitions must be positive')
        run(args.out.resolve(), args.binary, args.repetitions, args.project, args.label, args.timings, args.warnings_only, args.resume)
    else:
        if args.repetitions < 1:
            parser.error('repetitions must be positive')
        ab(args.out.resolve(), args.baseline, args.candidate, args.repetitions, args.project, args.label,
           args.baseline_env, args.candidate_env, args.resume)
