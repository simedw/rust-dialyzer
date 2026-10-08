#!/usr/bin/env python3
import hashlib
import itertools
import json
from pathlib import Path
import random
import subprocess
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.cache/snapshot_check'
OUT.mkdir(parents=True, exist_ok=True)
files = [ROOT / name for name in ['src/incremental.rs','src/invalidation.rs','src/bin/snapshot_check_probe.rs','scripts/snapshot_check_oracle.exs','Cargo.toml','Cargo.lock']] + [Path(__file__).resolve()]
def hashes():
    return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
implementation = hashes()
subprocess.run(['cargo','build','--release','--offline','--bin','snapshot_check_probe'],cwd=ROOT,check=True)
rows=[]
profile=dict(analysis_version='current',implementation={'erl_types':'one','erl_bif_types':'one'})
def emit(previous,current,tags):
    rows.append(dict(id=len(rows),previous=previous,current=current,profile=profile,legal_warnings=tags))
for version,impl,tags,available,old,current,newtags in itertools.product(['current','old'],[{}, {'erl_types':'one'},{'erl_types':'two'},{'erl_bif_types':'one','erl_types':'one'}],[None,[],['warn_return_no_exit'],['warn_return_no_exit','warn_not_called']],[False,True],[{}, {'a':'one'},{'a':'one','b':'one'}],[{}, {'a':'one'},{'a':'two'},{'a':'one','b':'one'}],[[],['warn_return_no_exit'],['warn_not_called','warn_return_no_exit']]):
    previous=dict(profile=dict(analysis_version=version,implementation=impl),modules=old,legal_warnings=tags,warnings_available=available,dependencies={})
    emit(previous,current,newtags)
for current in [{},{'a':'one'},{'a':'one','b':'two'}]:
    emit(None,current,[])
rng=random.Random(28306)
for _ in range(5000):
    def modules():
        return {f'm{i}':rng.choice(['one','two']) for i in range(50) if rng.randrange(2)}
    previous=dict(profile=profile,modules=modules(),legal_warnings=[],warnings_available=bool(rng.randrange(2)),dependencies={})
    emit(previous,modules(),[])
request=OUT/'requests.jsonl'
request.write_text(''.join(json.dumps(row)+'\n' for row in rows))
for command,name in [(['elixir','scripts/snapshot_check_oracle.exs'],'native'),([str(ROOT/'target/release/snapshot_check_probe')],'rust')]:
    with request.open() as inp,(OUT/f'{name}.jsonl').open('w') as out:
        subprocess.run(command,cwd=ROOT,stdin=inp,stdout=out,check=True)
native=[json.loads(line) for line in (OUT/'native.jsonl').read_text().splitlines()]
rust=[json.loads(line) for line in (OUT/'rust.jsonl').read_text().splitlines()]
if len(rows)!=7307 or len(native)!=len(rows) or len(rust)!=len(rows):
    raise RuntimeError('snapshot classification denominator changed')
failures=[dict(request=row,native=a,rust=b) for row,a,b in zip(rows,native,rust) if a!={k:b[k] for k in ['id','check']}]
if failures:
    (OUT/'failures.json').write_text(json.dumps(failures[:20],indent=2))
    raise RuntimeError(f'{len(failures)} snapshot classification differences')
if implementation!=hashes():
    raise RuntimeError('snapshot classification implementation changed during verification')
report=dict(commands=len(rows),matched=len(rows),complete_snapshot_classification=True,complete_disk_lifecycle=False,scope='Parsed header and compiler-supplied fingerprints; native hash and disk read boundaries supplied explicitly',implementation_sha256=implementation,inputs_sha256={str(request.relative_to(ROOT)):hashlib.sha256(request.read_bytes()).hexdigest()})
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({key:report[key] for key in ['commands','matched','complete_disk_lifecycle']}))
