#!/usr/bin/env python3
"""Verify that compiler preparation rejects an identity input changed after intake."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--root', type=Path, required=True)
p.add_argument('--prior', type=Path, required=True)
p.add_argument('--out', type=Path, required=True)
a = p.parse_args()
a.out.mkdir(parents=True, exist_ok=False)
env = os.environ.copy()
env['PATH'] = '/Users/simedw/.asdf/installs/erlang/28.3/bin:/Users/simedw/.asdf/installs/elixir/1.19.4-otp-28/bin:' + env['PATH']
with tempfile.TemporaryDirectory(prefix='rust-dialyzer-identity-proof-') as directory:
    directory = Path(directory)
    sentinel = directory / 'implementation-input'
    sentinel.write_text('before')
    files = [a.root / 'target/release/rust-dialyzer-poc', *sorted((a.root / 'scripts').glob('*.exs')), sentinel]
    script = a.root / 'scripts/compiled_bundle.exs'
    identity_script = a.root / 'scripts/compiler_identity_main.exs'
    def invoke(operation, request):
        request_path = directory / (operation + '-request.json')
        output_path = directory / (operation + '-output.json')
        request_path.write_text(json.dumps(request))
        run = subprocess.run(['elixir', str(identity_script if operation == 'identity' else script), operation, str(request_path), str(output_path)], env=env, capture_output=True, timeout=60)
        (a.out / (operation + '.stderr')).write_bytes(run.stderr)
        return run, output_path
    run, output = invoke('identity', dict(implementation_files=list(map(str, files)), implementation_sha256=''))
    assert run.returncode == 0, run.stderr.decode()
    identity = json.loads(output.read_text())
    sentinel.write_text('after')
    run, output = invoke('prepare', dict(implementation_files=list(map(str, files)), implementation_sha256=identity['implementation_sha256'], prior_plt=str(a.prior), beam_files=[]))
    assert run.returncode != 0
    assert 'implementation changed while preparing analysis' in run.stderr.decode(), run.stderr.decode()
    assert not output.exists()
    report = dict(source_mutation_rejected=True, prepared_output_not_created=True, implementation_inputs=identity['implementation_inputs'])
    (a.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))
