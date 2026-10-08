"""Run actual public mode comparisons with a complete candidate inventory."""
import argparse
import json
from pathlib import Path
import subprocess

from replay_composed_pipeline import sha


def inventory(root):
    paths = [root / 'Cargo.toml', root / 'Cargo.lock']
    for directory in ('src', 'scripts', 'corpus'):
        paths.extend(p for p in (root / directory).rglob('*') if p.is_file())
    for name in ('analysis_mode_probe', 'diagnostic_adapter_probe',
                 'target_mode_diagnostics_probe'):
        paths.append(root / 'target/release' / name)
    return {str(p): sha(p) for p in paths}


def main():
    parser = argparse.ArgumentParser()
    for name in ('root', 'checkpoint', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    root, checkpoint, out = (getattr(args, name).resolve()
        for name in ('root', 'checkpoint', 'out'))
    assert not out.exists(), 'use a fresh output directory'
    assert not out.is_relative_to(checkpoint), 'output overlaps frozen checkpoint'
    for directory in ('src', 'scripts', 'corpus', 'target'):
        assert not out.is_relative_to(root / directory), 'output overlaps candidate inputs'
    manifest = checkpoint / 'checkpoint.json'
    assert sha(manifest) == 'aa62140b02d9dd8fc58c930e1010b6ecce38c23e3e0fe51147368476b0d7fa86'
    frozen = {str(manifest): sha(manifest)}
    for name, expected in json.loads(manifest.read_text())['files'].items():
        path = (checkpoint / name).resolve()
        assert path.is_relative_to(checkpoint) and sha(path) == expected, name
        frozen[str(path)] = expected
    before = inventory(root)
    subprocess.run(['python3', str(checkpoint / 'replay.py'), str(root), str(out)],
                   cwd=root, check=True)
    assert before == inventory(root), 'candidate source, input inventory or binaries changed'
    assert frozen == {p: sha(Path(p)) for p in frozen}, 'frozen proof changed'
    report = json.loads((out / 'proof/mode-session-report.json').read_text())
    assert report['counts'] == {'public_mode': 32, 'success': 22,
        'target_mode_diagnostics_probe': 288, 'diagnostic_adapter_probe': 144}
    report['complete_candidate_inventory'] = before
    report['frozen_tool_inventory'] = frozen
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print({'checks': 464, 'actual_public_modes': 32,
           'source_and_inputs_unchanged': True})


if __name__ == '__main__':
    main()
