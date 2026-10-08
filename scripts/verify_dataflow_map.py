#!/usr/bin/env python3
import hashlib,json,random,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.cache/dataflow_map';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/name for name in ['src/dataflow_map.rs','src/labels.rs','src/lattice.rs','src/type_api.rs','src/native_order.rs','src/runtime_term.rs','src/presentation.rs','src/presentation_form.rs','src/bin/dataflow_map_probe.rs','scripts/dataflow_map_oracle.exs','scripts/type_codec.exs','scripts/type_wire.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
def hashes():return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
implementation=hashes();subprocess.run(['cargo','build','--release','--offline','--bin','dataflow_map_probe'],cwd=ROOT,check=True)
rng=random.Random(28306)
pool=[dict(kind=k) for k in ['any','none','unit','integer','atom','float','binary','nil','tuple_any','map_any']]+[dict(kind='atoms',values=['a']),dict(kind='atoms',values=['z']),dict(kind='integers',values=['1']),dict(kind='tuple',elements=[dict(kind='integer'),dict(kind='atom')])]
def steps(depth=0):
    result=[]
    for _ in range(rng.randrange(1,12)):
        key=rng.randrange(16);kind=rng.choice(['enter','literal','alias']+(['join'] if depth<3 else []))
        if kind=='join':result.append(dict(operation=kind,branches=[steps(depth+1) for _ in range(rng.randrange(4))]))
        elif kind=='alias':result.append(dict(operation=kind,key=key,value=rng.randrange(16)))
        else:result.append(dict(operation=kind,key=key,type=rng.choice(pool)))
    return result
rows=[dict(id=i,steps=steps()) for i in range(5000)]
rows += [dict(id=5000,steps=[dict(operation='enter',key=0,type=dict(kind='integer')),dict(operation='alias',key=1,value=0),dict(operation='join',branches=[[dict(operation='enter',key=1,type=dict(kind='atom'))],[dict(operation='enter',key=0,type=dict(kind='float'))]])]),dict(id=5001,steps=[dict(operation='enter',key=0,type=dict(kind='integer')),dict(operation='alias',key=1,value=0),dict(operation='join',branches=[])])]
request=OUT/'requests.jsonl';request.write_text(''.join(json.dumps(row)+'\n' for row in rows))
for command,name in [(['elixir','scripts/dataflow_map_oracle.exs'],'native'),([str(ROOT/'target/release/dataflow_map_probe')],'rust')]:
    with request.open() as inp,(OUT/f'{name}.jsonl').open('w') as out:subprocess.run(command,cwd=ROOT,stdin=inp,stdout=out,check=True)
native=[json.loads(line) for line in (OUT/'native.jsonl').read_text().splitlines()];rust=[json.loads(line) for line in (OUT/'rust.jsonl').read_text().splitlines()]
if len(rows)!=5002 or len(native)!=len(rows) or len(rust)!=len(rows):raise RuntimeError('dataflow map denominator changed')
failures=[dict(request=row,native=x,rust=y) for row,x,y in zip(rows,native,rust) if x!=y]
if failures:
    (OUT/'failures.json').write_text(json.dumps(failures[:20],indent=2));raise RuntimeError(f'{len(failures)} dataflow map differences')
if implementation!=hashes():raise RuntimeError('dataflow map implementation changed during verification')
report=dict(commands=len(rows),matched=len(rows),complete_environment_algebra=True,complete_core_node_adapter=False,complete_pattern_binding=False,scope='Label type/substitution map, raw literal storage, nested branch scopes and native changed-key joins',implementation_sha256=implementation,inputs_sha256={str(request.relative_to(ROOT)):hashlib.sha256(request.read_bytes()).hexdigest()})
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k] for k in ['commands','matched','complete_pattern_binding']}))
