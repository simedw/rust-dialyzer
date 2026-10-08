#!/usr/bin/env python3
"""Recreate the vendored frontend from a pristine pinned Firefly checkout."""
import argparse, pathlib, shutil, json, subprocess
p=argparse.ArgumentParser();p.add_argument('upstream');p.add_argument('destination');args=p.parse_args()
base=pathlib.Path(__file__).resolve().parent;src=pathlib.Path(args.upstream).resolve();dst=pathlib.Path(args.destination).resolve()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=src,text=True).strip()=='c672f3ad5bebd3d6e9e2fe6056a154ed92fd1d07'
assert not dst.exists(), 'Destination must be fresh'
paths=json.loads((base/'dependency-closure.json').read_text())
for d in paths:shutil.copytree(src/d,dst/d)
for name in ('Cargo.toml','Cargo.lock','LICENSE.md','NOTICE','modernization.patch','modernization-files.json','dependency-closure.json'):shutil.copy2(base/name,dst/name)
shutil.copytree(base/'probe',dst/'probe')
shutil.copytree(base/'fixtures',dst/'fixtures')
subprocess.run(['patch','-p1','-i',str(dst/'modernization.patch')],cwd=dst,check=True)
print('Build explicitly using: RUSTC_BOOTSTRAP=1 cargo build --offline -p firefly-audit-probe')
