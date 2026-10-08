#!/usr/bin/env python3
"""Build the prototype and capture explicit OTP oracle context for verification."""
import argparse
import json
import os
from pathlib import Path
import subprocess

base = Path(__file__).resolve().parent
root = base.parents[1]
p = argparse.ArgumentParser()
p.add_argument('--cargo', default='cargo', help='Cargo executable; explicit path avoids toolchain shim environment resets')
p.add_argument('--rustc', help='Optional actual rustc executable to bypass environment-resetting shims')
p.add_argument('--offline', action='store_true')
p.add_argument('--bundled-fixtures', action='store_true', help='Compile included fixture sources instead of using unchanged corpus BEAMs')
a = p.parse_args()
cache = root/'.cache/corpus/otp/base.plt'
if not cache.is_file():
    raise SystemExit('Missing existing corpus base PLT: '+str(cache))
env = dict(os.environ, RUSTC_BOOTSTRAP='1')
if a.rustc:
    env['RUSTC'] = a.rustc
flags = ['--offline'] if a.offline else []
for directory in (root, base/'input', base/'adapter'):
    command = [a.cargo, 'build', *flags]
    if directory == root:
        command += ['--bin', 'compiled_analysis_probe']
    subprocess.run(command, cwd=directory, env=env, check=True)
harness = base/'harness'
(harness/'beams').mkdir(exist_ok=True)
for script in ('generate.exs','generate_corpus.exs'):
    command = ['elixir',str(harness/script),str(root),str(harness)]
    if script == 'generate_corpus.exs' and a.bundled_fixtures:
        command.append('--bundled-fixtures')
    subprocess.run(command,check=True)
# Capture a baseline from OTP-generated executable Core, separately from the native run.
with (harness/'corpus/oracle-row.jsonl').open() as incoming, (harness/'corpus/oracle-rust-result.jsonl').open('w') as outgoing:
    subprocess.run([str(root/'target/debug/compiled_analysis_probe'),str(harness/'prior-library.json'),str(harness/'closure-profile.json')],stdin=incoming,stdout=outgoing,check=True)
baseline = [json.loads(line) for line in (harness/'corpus/oracle-rust-result.jsonl').read_text().splitlines()]
assert len(baseline) == 13 and all(row.get('public_warnings_match') is True for row in baseline), 'OTP Core analyzer baseline failed'
print('Prepared 13 oracle cases; execute python3 run/run_corpus.py '+str(root))
