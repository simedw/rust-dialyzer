#!/usr/bin/env python3
"""Coverage census, not a performance benchmark. No compiler oracle is invoked."""
import argparse, collections, json, pathlib, subprocess, tempfile, time

p = argparse.ArgumentParser()
p.add_argument('binary', type=pathlib.Path)
p.add_argument('manifest', type=pathlib.Path)
p.add_argument('output', type=pathlib.Path)
p.add_argument('--through', choices=['inspect','forms','core','clean'], default='forms')
a = p.parse_args()
started = time.monotonic()
rows = []
stages = ['inspect','forms','core','clean'][:['inspect','forms','core','clean'].index(a.through)+1]
with tempfile.TemporaryDirectory(prefix='native-beam-census-') as directory:
    for source in a.manifest.read_text().splitlines():
        row = {'beam':source, 'oracle_verified':False, 'stages':{}}
        for stage in stages:
            cmd = [str(a.binary),stage,source]
            if stage != 'inspect': cmd.append(str(pathlib.Path(directory)/'output.etf'))
            result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
            if result.returncode:
                row['stages'][stage] = {'status':'error','exit_code':result.returncode,'error':result.stderr.strip()}
                break
            row['stages'][stage] = {'status':'produced_unverified','metrics':json.loads(result.stdout)}
        rows.append(row)
counts = {stage:dict(collections.Counter(row['stages'].get(stage,{}).get('status','not_reached') for row in rows)) for stage in stages}
a.output.parent.mkdir(parents=True,exist_ok=True)
a.output.write_text(json.dumps({'manifest':str(a.manifest),'through':a.through,'modules':len(rows),'elapsed_seconds':time.monotonic()-started,'counts':counts,'rows':rows},indent=2)+'\n')
print(json.dumps(counts))
