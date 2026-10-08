#!/usr/bin/env python3
import hashlib
import itertools
import json
from pathlib import Path
import random
import subprocess
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.cache/callbacks';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/name for name in ['src/behaviours.rs','src/call_transfer.rs','src/contract_runtime.rs','src/lattice.rs','src/type_api.rs','src/native_order.rs','src/runtime_term.rs','src/presentation.rs','src/presentation_form.rs','src/bin/callback_probe.rs','scripts/callback_oracle.exs','scripts/type_codec.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
def hashes():return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
implementation=hashes()
subprocess.run(['cargo','build','--release','--offline','--bin','callback_probe'],cwd=ROOT,check=True)
a=lambda kind:dict(kind=kind)
fun=lambda args,ret:dict(kind='function',domain=args,range=ret)
nominal=lambda opacity:dict(kind='nominal',name=dict(module='owner',name='x',arity=0,opacity=opacity),inner=a('integer'))
pool=[a(k) for k in ['any','none','unit','integer','float','number','atom','binary','nil','tuple_any','map_any','identifier']]+[dict(kind='atoms',values=['ok']),dict(kind='atoms',values=['bad']),dict(kind='integers',values=['1']),dict(kind='range',**{'from':'1','to':'10'}),dict(kind='bitstring',unit='3',base='2'),dict(kind='tuple',elements=[dict(kind='atoms',values=['ok']),a('integer')]),dict(kind='proper_list',elements=a('integer'),nonempty=False),dict(kind='proper_list',elements=a('atom'),nonempty=True),fun([a('integer')],a('atom')),dict(kind='identifiers',values=['pid']),nominal('opaque'),nominal('transparent')]
assert len(pool)==24
rows=[]
def emit(callback,inferred,declared=None,optional=False,exported=True):rows.append(dict(id=len(rows),callback=callback,inferred=inferred,declared=declared,optional=optional,exported=exported))
for left,right,spec,optional,exported in itertools.product(pool,pool,[False,True],[False,True],[False,True]):
    emit(fun([left,right],left),dict(arguments=[right,left],result=right),fun([right,left],right) if spec else None,optional,exported)
for ret,optional,exported in itertools.product(pool,[False,True],[False,True]):emit(fun([a('any')],ret),None,None,optional,exported)
rng=random.Random(28306)
for _ in range(5000):
    arity=rng.randrange(5)
    def signature():return fun([rng.choice(pool) for _ in range(arity)],rng.choice(pool))
    callback=signature();inferred=signature();spec=signature() if rng.randrange(2) else None
    emit(callback,dict(arguments=inferred['domain'],result=inferred['range']),spec,bool(rng.randrange(2)),bool(rng.randrange(2)))
request=OUT/'requests.jsonl';request.write_text(''.join(json.dumps(row)+'\n' for row in rows))
for command,name in [(['elixir','scripts/callback_oracle.exs'],'native'),([str(ROOT/'target/release/callback_probe')],'rust')]:
    with request.open() as inp,(OUT/f'{name}.jsonl').open('w') as out:subprocess.run(command,cwd=ROOT,stdin=inp,stdout=out,check=True)
native=[json.loads(line) for line in (OUT/'native.jsonl').read_text().splitlines()];rust=[json.loads(line) for line in (OUT/'rust.jsonl').read_text().splitlines()]
if len(rows)!=9704 or len(native)!=len(rows) or len(rust)!=len(rows):raise RuntimeError('callback denominator changed')
failures=[dict(request=row,native=x,rust=y) for row,x,y in zip(rows,native,rust) if x!=y]
if failures:
    (OUT/'failures.json').write_text(json.dumps(failures[:20],indent=2));raise RuntimeError(f'{len(failures)} callback differences')
if implementation!=hashes():raise RuntimeError('callback implementation changed during verification')
report=dict(commands=len(rows),matched=len(rows),complete_callback_type_checks=True,complete_behaviour_metadata_and_origins=False,implementation_sha256=implementation,inputs_sha256={str(request.relative_to(ROOT)):hashlib.sha256(request.read_bytes()).hexdigest()})
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k] for k in ['commands','matched','complete_behaviour_metadata_and_origins']}))
