#!/usr/bin/env python3
import hashlib,json,random,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/success_phase';OUT.mkdir(parents=True,exist_ok=True)
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
files=[ROOT/p for p in ['src/success_phase.rs','src/bin/success_phase_probe.rs','src/lattice.rs','src/type_api.rs','src/call_transfer.rs','src/native_order.rs','src/runtime_term.rs','src/presentation.rs','src/presentation_form.rs','scripts/type_codec.exs','scripts/type_wire.exs','scripts/success_phase_oracle.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
hashes={str(p.relative_to(ROOT)):sha(p) for p in files};rng=random.Random(1109)
scalar=lambda k:{'kind':k}
pool=[scalar(k)for k in ['any','none','unit','integer','number','atom','float','nil','tuple_any']]+[{'kind':'atoms','values':['a']},{'kind':'atoms','values':['a','b']}]
for depth in range(1,8):
 value=scalar('integer')
 for _ in range(depth):value={'kind':'tuple','elements':[{'kind':'atoms','values':['tag']},value]}
 pool.append(value)
 pool.append({'kind':'list','elements':value,'tail':scalar('nil'),'nonempty':bool(depth%2)})
rows=[]
for i in range(5000):
 count=rng.randrange(9);old=[];new=[]
 for key in range(count):
  arity=rng.randrange(4);before={'label':key,'arguments':[rng.choice(pool)for _ in range(arity)],'result':rng.choice(pool)}
  after=before if rng.random()<.4 else {'label':key,'arguments':[rng.choice(pool)for _ in range(arity)],'result':rng.choice(pool)}
  old.append(before);new.append(after)
 if i%5==0 and old:old.pop(rng.randrange(len(old)))
 if i%7==0 and new:new.pop(rng.randrange(len(new)))
 rows.append({'id':i,'previous':old,'next':new})
request=OUT/'requests.jsonl';request.write_text(''.join(json.dumps(row)+'\n'for row in rows))
subprocess.run(['cargo','build','--release','--offline','--bin','success_phase_probe'],cwd=ROOT,check=True)
for cmd,name in [(['elixir','scripts/success_phase_oracle.exs'],'native'),([str(ROOT/'target/release/success_phase_probe')],'rust')]:
 with request.open()as inp,(OUT/f'{name}.jsonl').open('w')as out:subprocess.run(cmd,cwd=ROOT,stdin=inp,stdout=out,check=True)
native=[json.loads(s)for s in (OUT/'native.jsonl').read_text().splitlines()];rust=[json.loads(s)for s in (OUT/'rust.jsonl').read_text().splitlines()]
if len(native)!=5000 or len(rust)!=5000 or [r['id']for r in native]!=list(range(5000)) or [r.get('id')for r in rust]!=list(range(5000)):raise RuntimeError('outcome identities changed')
failures=[{'request':r,'native':n,'rust':s}for r,n,s in zip(rows,native,rust)if n!=s]
if failures:(OUT/'failures.json').write_text(json.dumps(failures[:20],indent=2));raise RuntimeError(f'{len(failures)} phase comparison differences')
if hashes!={str(p.relative_to(ROOT)):sha(p)for p in files}:raise RuntimeError('implementation changed during proof')
source=ROOT/'.cache/upstream/otp/lib/dialyzer/src/dialyzer_succ_typings.erl'
report=dict(commands=5000,matched=5000,complete_phase_comparison=True,complete_production_orchestration=False,implementation_sha256=hashes,inputs_sha256={str(source.relative_to(ROOT)):sha(source)},unsupported=['SCC success generation','module dataflow refinement','contract activation and phase orchestration'])
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k]for k in ['commands','matched','complete_production_orchestration']}))
