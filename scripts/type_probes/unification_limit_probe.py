import json,random,subprocess
from pathlib import Path
from probe_config import configuration, read_commands
WORKSPACE=Path(__file__).resolve().parents[2];CONFIG=configuration(WORKSPACE);root=CONFIG.out;r=random.Random(28316)
values={}
for line in read_commands(root/'subst_commands.jsonl', CONFIG):
 a=json.loads(line)['a'];values[json.dumps(a,sort_keys=True)]=a
pool=r.sample(list(values.values()),500);commands=[]
for t in pool:
 malformed=dict(kind='unification_nominal_set',nominal=dict(kind='nominal_raw',name=dict(symbol='a'),inner=t),structural=t)
 for binding in [t,malformed]:
  for depth in [-1,0,1,2,5]:commands.append(dict(op='unification_limit',a=dict(kind='none'),binding=binding,depth=depth))
p=root/'unification_limit_commands.jsonl';out=root/'unification_limit_results.jsonl';p.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with p.open('rb')as src,out.open('wb')as dest:subprocess.run([str(CONFIG.binary)],stdin=src,stdout=dest,check=True)
subprocess.run(['elixir',str(WORKSPACE/'scripts/type_oracle.exs'),str(p),str(out)],check=True)
