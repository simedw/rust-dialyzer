#!/usr/bin/env python3
"""Replay raw dependency values and full dictionaries through Original readers."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def rows(path):
    return [json.loads(line) for line in path.read_text().splitlines()]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--checkpoint', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    frozen, out = args.checkpoint.resolve(), args.out.resolve()
    assert not out.exists(), 'use a fresh output directory'
    manifest = frozen / 'manifest.json'
    assert digest(manifest) == 'bd32787ef9f40b6d735ee3f0deeccb2e17621877bf2bb7757a63d8b9244d59b8'
    files = json.loads(manifest.read_text())
    for name, expected in files.items():
        assert digest(frozen / name) == expected, name
    native = Path('/Users/simedw/.asdf/installs/erlang/28.3')
    elixir = Path('/Users/simedw/.asdf/installs/elixir/1.19.4-otp-28/bin/elixir')
    paths = [manifest, Path(__file__), root / 'Cargo.toml', root / 'Cargo.lock', elixir]
    for directory, suffix in [('src', '*.rs'), ('scripts', '*'), ('corpus', '*.json')]:
        paths.extend(p for p in (root / directory).rglob(suffix) if p.is_file())
    paths.extend(frozen / name for name in files)
    paths.extend(p for p in (native / 'lib').rglob('*')
                 if p.is_file() and p.suffix in ('.erl', '.beam'))
    paths.extend((native / 'erts-16.2/bin').glob('beam.smp'))
    semantic = root / 'target/release/classic_checked_probe'
    physical = root / 'target/release/raw_dependency_persistence_probe'
    paths.extend([semantic, physical])

    def inventory():
        return {str(p): digest(p) for p in sorted(set(paths))}

    before = inventory()
    out.mkdir(parents=True)
    subprocess.run(['python3', str(frozen / 'verify_values.py'), '--binary', str(semantic),
                    '--out', str(out / 'values')], check=True)
    raw = out / 'dictionary-etf.jsonl'
    source = frozen / 'native-physical.jsonl'
    with source.open('rb') as stdin, raw.open('wb') as stdout:
        subprocess.run([str(physical)], stdin=stdin, stdout=stdout, check=True)
    read = out / 'original-reader.jsonl'
    env = os.environ.copy()
    env['PATH'] = str(native / 'bin') + os.pathsep + str(elixir.parent) + os.pathsep + env['PATH']
    subprocess.run([str(elixir), str(frozen / 'read_physical.exs'), str(frozen),
                    str(raw), str(read)], env=env, check=True)
    expected, actual = rows(source), rows(read)
    assert len(expected) == len(actual) == 72
    for left, right in zip(expected, actual):
        assert left['id'] == right['id']
        assert left['expected_dict'] == right['actual_dict'], left['id']
    after = inventory()
    assert before == after, 'guarded artifact changed'
    report = dict(semantic_raw_values=582, actual_original_dictionary_readers=72,
                  entire_records_exact=True, sources_binaries_inputs_unchanged=True,
                  public_backend_workflows=False)
    (out / 'provenance.json').write_text(json.dumps(dict(before=before, after=after), indent=2) + '\n')
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(report)


if __name__ == '__main__':
    main()
