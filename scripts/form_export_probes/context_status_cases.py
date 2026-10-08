import json,argparse,subprocess
from pathlib import Path
from cache_fixture_lib import Atom as A,arena,remote,user,builtin,table,integer
scripts=Path(__file__).resolve().parent;root=scripts.parents[1];p=root/'.cache/form_exports';p.mkdir(parents=True,exist_ok=True);parser=argparse.ArgumentParser();parser.add_argument('--probe',default=str(p/'target/release/cache_probe'));args=parser.parse_args()
cases=[]
for modules,exports,form in [({A('m'):{}},[(A('m'),A('missing'),0)],remote('m','missing')),({A('m'):{}},[],remote('m','missing')),({},[(A('m'),A('missing'),0)],remote('m','missing')),({A('m'):table('m',1)},[],remote('m','a')),({A('m'):table('m',1)},[(A('m'),A('a'),0)],remote('m','a'))]:
 for mode in ['resolve','check_remote','without_remote']:
  for kind in ['spec','check','type','record']:
   cases.append(dict(snapshots=[arena(modules)],exports=arena(exports),steps=[dict(snapshot=0,form=arena(form),mode=mode,site=dict(kind=kind,module='m',name='a',arity=0))]))
with(p/'context_status_commands.jsonl').open('w')as f:
 for c in cases:f.write(json.dumps(c)+'\n')
with(p/'context_status_commands.jsonl').open('rb')as i,(p/'context_status_results.jsonl').open('wb')as o:subprocess.run([args.probe],stdin=i,stdout=o,check=True)
subprocess.run(['elixir',str(scripts/'context_status_oracle.exs'),str(p/'context_status_commands.jsonl'),str(p/'context_status_results.jsonl')],check=True)
