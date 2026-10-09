#!/usr/bin/env python3
"""Build PGO from both full workloads. Run under lockf -k rust-dialyzer-run.lock."""
import argparse
import collections
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

from compare_warnings import expand, items

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('output', type=Path)
parser.add_argument('--source', type=Path, default=ROOT)
parser.add_argument('--rustc', default='rustc')
parser.add_argument('--cargo', type=Path)
parser.add_argument('--cargo-home', type=Path, required=True)
parser.add_argument('--llvm-profdata', type=Path, required=True)
parser.add_argument('--keep-target', action='store_true')
args = parser.parse_args()
source = args.source.resolve()
out = args.output.resolve()
assert not out.exists(), 'Output must be new; stale profiles cannot be reused'
rust_version = subprocess.check_output([args.rustc, '-vV'], text=True)
target = re.search(r'^host: (.+)$', rust_version, re.M).group(1)
rust_llvm = re.search(r'^LLVM version: (\d+)', rust_version, re.M).group(1)
prof_version = subprocess.check_output([str(args.llvm_profdata), '--version'], text=True)
assert re.search(r'LLVM version (\d+)', prof_version).group(1) == rust_llvm, \
    'llvm-profdata and rustc must use the same LLVM major version'
sysroot = Path(subprocess.check_output([args.rustc, '--print', 'sysroot'], text=True).strip())
cargo = args.cargo.resolve() if args.cargo else sysroot / 'bin/cargo'
rustc = sysroot / 'bin/rustc'
out.mkdir(parents=True)
raw = out / 'raw'
raw.mkdir()
env = dict(os.environ, CARGO_HOME=str(args.cargo_home.resolve()),
           CARGO_TARGET_DIR=str(out / 'target'), RUSTC=str(rustc),
           ERL_FLAGS='+S 14:14')
for key in ['RUST_DIALYZER_TIMINGS', 'RUST_DIALYZER_VERIFY_FASTPATH',
            'RUST_DIALYZER_SEQUENTIAL', 'LLVM_PROFILE_FILE', 'CARGO_ENCODED_RUSTFLAGS']:
    env.pop(key, None)
metadata = {
    'rustc': rust_version, 'llvm_profdata': prof_version, 'target': target,
    'source_sha256': {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest()
                      for p in [source / 'Cargo.toml', source / 'Cargo.lock',
                                *sorted((source / 'src').rglob('*.rs'))]},
    'training_workloads': ['phoenix', 'elixir'], 'proofs': [],
}


def run(label, command, environment=env, cwd=ROOT):
    with (out / f'{label}.stdout').open('w') as stdout, \
            (out / f'{label}.stderr').open('w') as stderr:
        subprocess.run(list(map(str, command)), cwd=cwd, env=environment,
                       stdout=stdout, stderr=stderr, check=True)
    print(json.dumps({'completed': label}), flush=True)


def build(label, flags):
    run(label, [cargo, 'build', '--release', '--offline', '--target', target,
                '--bin', 'core_cache_analyze'], dict(env, RUSTFLAGS=flags), source)
    metadata[label + '_flags'] = flags


def signatures(value):
    rows = value['signatures']
    indexed = {(s['module'], s['function'], s['arity']): s for s in rows}
    assert len(indexed) == len(rows), 'duplicate inferred signature'
    return indexed


def warnings(value):
    return collections.Counter(json.dumps(row, sort_keys=True)
                               for row in items(expand(value['warnings'])))


binary = out / 'target' / target / 'release/core_cache_analyze'
build('generate', '-Cprofile-generate=' + str(raw))
instrumented = out / 'instrumented-binary'
shutil.copy2(binary, instrumented)
instrumented.chmod(0o555)
for project in metadata['training_workloads']:
    inventory = ROOT / 'reports/head-to-head-2026-10-09' / project
    cache = out / (project + '-cache')
    manifest = out / (project + '-manifest.json')
    run(project + '-export', [ROOT / '.cache/real-projects/elixir-v1.20.4/bin/elixir',
        ROOT / 'prototypes/core_cache/core_cache.exs', ROOT, inventory / 'beams.txt',
        cache, manifest, '--jobs', '14'])
    details = json.loads((out / f'{project}-export.stdout').read_text().splitlines()[-1])
    assert details['compiled'] == details['modules'] and details['reused'] == 0
    run(project + '-train', [instrumented, ROOT / '.cache/phase0/rust-cone/out/prior-library.json',
        manifest, inventory / 'options.json'],
        dict(env, LLVM_PROFILE_FILE=str(raw / '%m-%p.profraw')))
    actual = json.loads((out / f'{project}-train.stdout').read_text())
    reference = json.loads((inventory / 'run-1/rust.stdout').read_text())
    assert actual['status'] == 'ok'
    assert signatures(actual) == signatures(reference), project + ': signatures changed'
    assert warnings(actual) == warnings(reference), project + ': warnings changed'
    metadata['proofs'].append({'project': project, 'modules': actual['modules'],
        'warnings': actual['public_warning_count'], 'signatures': len(actual['signatures']),
        'exact': True})
    for attempt in range(3):
        try:
            shutil.rmtree(cache)
            break
        except OSError as error:
            if error.errno != errno.ENOTEMPTY or attempt == 2:
                raise

profiles = sorted(raw.glob('*.profraw'))
assert profiles and all(p.stat().st_size for p in profiles), 'No usable training profiles'
profile = out / 'merged.profdata'
run('merge', [args.llvm_profdata, 'merge', '-o', profile, *profiles])
run('profile-summary', [args.llvm_profdata, 'show', profile])
build('use', '-Cprofile-use=' + str(profile) + ' -Cllvm-args=-pgo-warn-missing-function')
optimized = out / 'core_cache_analyze'
shutil.copy2(binary, optimized)
optimized.chmod(0o555)
metadata['binary_sha256'] = hashlib.sha256(optimized.read_bytes()).hexdigest()
metadata['profile_sha256'] = hashlib.sha256(profile.read_bytes()).hexdigest()
metadata['note'] = 'Training is validated; benchmark and full parity gate are separate required steps.'
(out / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
if not args.keep_target:
    shutil.rmtree(out / 'target')
print(json.dumps({'binary': str(optimized), 'metadata': str(out / 'metadata.json')}), flush=True)
