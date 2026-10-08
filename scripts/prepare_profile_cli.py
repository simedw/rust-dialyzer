#!/usr/bin/env python3
"""Recreate the instrumented development CLI from its verified source baseline."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--baseline-root', type=Path, required=True)
p.add_argument('--out', type=Path, required=True)
a = p.parse_args()
root = Path(__file__).resolve().parents[1]
expected = json.loads((root/'scripts/profile_cli_baseline.json').read_text())
for name,digest in expected.items():
    assert hashlib.sha256((a.baseline_root/name).read_bytes()).hexdigest() == digest, f'baseline changed: {name}'
a.out.mkdir(parents=True, exist_ok=False)
for directory in ['src','scripts','corpus']:
    shutil.copytree(a.baseline_root/directory,a.out/directory)
for name in ['Cargo.toml','Cargo.lock']:
    shutil.copy2(a.baseline_root/name,a.out/name)
shutil.copy2(a.baseline_root/'target/release/rust-dialyzer-poc',a.out/'baseline-cli')
subprocess.run(['patch','--batch','-p1','-i',str(root/'scripts/profile_cli_candidate.patch')],cwd=a.out,check=True)
print(json.dumps({'candidate_root':str(a.out.resolve()),'baseline':str((a.out/'baseline-cli').resolve()),'build':['cargo','build','--offline','--locked','--release','--bin','rust-dialyzer-poc'],'build_environment':{'CARGO_INCREMENTAL':'0'}}))
