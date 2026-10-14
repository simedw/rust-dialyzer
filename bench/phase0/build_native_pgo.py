#!/usr/bin/env python3
"""Train PGO on frozen native workloads; training times are not acceptance times."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

from storage import require_disk_reserve
from three_projects import check_same, digest, measure, warning_set, write

ROOT = Path(__file__).resolve().parents[2]
# Name of the private large benchmark project; set locally.
LARGE_APP = os.environ.get('RUST_DIALYZER_LARGE_APP', 'large-app')


def source_hashes():
    paths = [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', *sorted((ROOT / 'src').rglob('*.rs'))]
    return {str(p.relative_to(ROOT)): digest(p) for p in paths}


def train(args):
    out, frozen = args.output.resolve(), args.frozen.resolve()
    assert not out.exists(), 'Use a new output directory; never mix profile generations'
    require_disk_reserve(out)
    rust = subprocess.check_output(['rustc', '-vV'], text=True)
    llvm = subprocess.check_output([args.llvm_profdata, '--version'], text=True)
    assert re.search(r'LLVM version: (\d+)', rust).group(1) == re.search(r'LLVM version (\d+)', llvm).group(1)
    target = re.search(r'^host: (.+)$', rust, re.M).group(1)
    out.mkdir(parents=True)
    raw = out / 'raw'
    raw.mkdir()
    metadata = {'rustc': rust, 'llvm_profdata': llvm, 'target': target,
                'source_sha256': source_hashes(), 'training': [],
                'note': 'Native frontend, empty prior, all inputs inferred. Full output parity required. Training wall times include instrumentation and are not speed evidence.'}
    write(out / 'metadata.json', metadata)
    env = dict(os.environ, CARGO_TARGET_DIR=str(out / 'target'), RUST_DIALYZER_SETUP_JOBS='14')
    for key in ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'LLVM_PROFILE_FILE',
                'RUST_DIALYZER_TIMINGS', 'RUST_DIALYZER_VERIFY_FASTPATH', 'RUST_DIALYZER_SEQUENTIAL',
                'RUST_DIALYZER_GENERATION_PROBE', 'RUST_DIALYZER_JOB_TRACE', 'RUST_DIALYZER_JOBTRACE',
                'RUST_DIALYZER_MEMO_DEPOT']:
        env.pop(key, None)

    def run(label, command, environment):
        require_disk_reserve(out)
        with (out / (label + '.stdout')).open('w') as stdout, (out / (label + '.stderr')).open('w') as stderr:
            subprocess.run(list(map(str, command)), cwd=ROOT, env=environment,
                           stdout=stdout, stderr=stderr, check=True)
        print(json.dumps({'completed': label}), flush=True)

    def build(label, flags):
        assert source_hashes() == metadata['source_sha256'], 'Source changed during training'
        run(label, ['cargo', 'build', '--release', '--offline', '--target', target,
                    '--bin', 'core_cache_analyze', '--bin', 'rust-dialyzer'], dict(env, RUSTFLAGS=flags))
        metadata[label + '_flags'] = flags

    build('generate', '-Cprofile-generate=' + str(raw))
    instrumented = out / 'instrumented-core_cache_analyze'
    shutil.copyfile(out / 'target' / target / 'release/core_cache_analyze', instrumented)
    instrumented.chmod(0o555)
    weights = []
    for project in ['elixir', 'phoenix', LARGE_APP]:
        inventory = frozen / project
        for entry in json.loads((inventory / 'inputs.json').read_text()):
            assert digest(entry['frozen']) == entry['sha256'], entry['frozen']
        options = inventory / ('options-full.json' if project == LARGE_APP else 'options.json')
        reference = (args.references / 'node-outline-full-large/native.stdout' if project == LARGE_APP
                     else frozen / 'node-outline-ab' / project / '0/candidate.stdout')
        if not reference.exists():
            reference = reference.with_name(reference.name + '.gz')
        assert reference.exists(), reference
        for mode in (['full', 'warnings'] if project != LARGE_APP else ['full']):
            directory = out / (project + '-' + mode)
            directory.mkdir()
            label = project + '-' + mode
            environment = dict(env, LLVM_PROFILE_FILE=str(raw / (label + '-%m-%p.profraw')))
            command = [instrumented, frozen / 'empty-prior.json', inventory / 'manifest.json', options]
            if mode == 'warnings':
                command.append('--warnings-only')
            metrics = measure(command, directory, 'train', environment)
            if mode == 'full':
                proof = check_same(reference, directory / 'train.stdout')
            else:
                from compare_signatures import index_result
                _, before = index_result(reference)
                after = json.loads((directory / 'train.stdout').read_text())
                assert after['status'] == 'ok' and before['modules'] == after['modules']
                assert warning_set(before) == warning_set(after)
                proof = {'modules': after['modules'], 'warnings': after['public_warning_count'],
                         'exact_warning_parity': True, 'signatures': 'full mode checked separately'}
            profiles = sorted(raw.glob(label + '-*.profraw'))
            assert profiles and all(p.stat().st_size for p in profiles)
            weight = 1 if project == LARGE_APP else 4
            weights.extend('--weighted-input=' + str(weight) + ',' + str(p) for p in profiles)
            metadata['training'].append({'project': project, 'mode': mode, 'weight': weight,
                                         'metrics': metrics, 'proof': proof,
                                         'options_sha256': digest(options), 'reference': str(reference)})
            write(directory / 'proof.json', proof)
            write(out / 'metadata.json', metadata)
            print(json.dumps({'completed': label, 'proof': proof}), flush=True)
    profile = out / 'merged.profdata'
    run('merge', [args.llvm_profdata, 'merge', '-o', profile, *weights], env)
    run('profile-summary', [args.llvm_profdata, 'show', profile], env)
    build('use', '-Cprofile-use=' + str(profile) + ' -Cllvm-args=-pgo-warn-missing-function')
    metadata['binary_sha256'] = {}
    for name in ['core_cache_analyze', 'rust-dialyzer']:
        shutil.copyfile(out / 'target' / target / 'release' / name, out / name)
        (out / name).chmod(0o555)
        metadata['binary_sha256'][name] = digest(out / name)
    metadata['profile_sha256'] = digest(profile)
    assert source_hashes() == metadata['source_sha256']
    write(out / 'metadata.json', metadata)
    if not args.keep_target:
        shutil.rmtree(out / 'target')
    print(json.dumps({'candidate': str(out / 'core_cache_analyze'), 'acceptance_complete': False}), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--frozen', type=Path, required=True)
    parser.add_argument('--references', type=Path, required=True)
    parser.add_argument('--llvm-profdata', type=Path, required=True)
    parser.add_argument('--keep-target', action='store_true')
    args = parser.parse_args()
    with open('/private/tmp/rust-dialyzer-run.lock', 'a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        train(args)
