#!/usr/bin/env python3
"""Run the retained Core census serially with frozen inputs and host-load records."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import shutil
import sys

ROOT = Path(__file__).resolve().parents[1]
# Name of the private large benchmark project; set locally.
LARGE_APP = os.environ.get('RUST_DIALYZER_LARGE_APP', 'large-app')
sys.path.insert(0, str(ROOT / 'bench' / 'phase0'))
from three_projects import digest, measure, write
from storage import require_disk_reserve


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('frozen', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/core_storage_probe')
    parser.add_argument('--project', action='append')
    args = parser.parse_args()
    with open('/private/tmp/rust-dialyzer-run.lock', 'a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        require_disk_reserve(args.output)
        args.output.mkdir(parents=True, exist_ok=False)
        binary = args.output.resolve() / 'core_storage_probe'
        shutil.copyfile(args.binary, binary)
        binary.chmod(0o555)
        env = dict(os.environ, RUST_DIALYZER_SETUP_JOBS='14')
        for key in ['RUST_DIALYZER_TIMINGS', 'RUST_DIALYZER_VERIFY_FASTPATH',
                    'RUST_DIALYZER_GENERATION_PROBE', 'RUST_DIALYZER_SEQUENTIAL']:
            env.pop(key, None)
        write(args.output / 'metadata.json', {
            'binary_sha256': digest(binary),
            'source_sha256': {str(p.relative_to(ROOT)): digest(p) for p in
                              [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', *sorted((ROOT / 'src').rglob('*.rs'))]},
            'scope': 'Retained Core storage diagnostic, not an inference or speed acceptance benchmark',
            'setup_jobs': 14,
        })
        summaries = {}
        for project in args.project or ['elixir', 'phoenix', LARGE_APP]:
            inventory = args.frozen / project
            for entry in json.loads((inventory / 'inputs.json').read_text()):
                assert digest(entry['frozen']) == entry['sha256'], entry['frozen']
            directory = args.output / project
            directory.mkdir()
            metrics = measure([binary, (inventory / 'manifest.json').resolve()], directory, 'storage', env)
            census = json.loads((directory / 'storage.stdout').read_text())
            summaries[project] = {'census': census, 'metrics': metrics}
            write(args.output / 'summary.json', summaries)
            print(project, json.dumps(census), flush=True)


if __name__ == '__main__':
    main()
