import json,random,subprocess
from pathlib import Path
WORKSPACE=Path(__file__).resolve().parents[2];root=WORKSPACE/".cache/types";r=random.Random(28311)
values={}
for file in ['nominal_commands.jsonl','raw_commands.jsonl','subst_commands.jsonl']:
 for line in (root/file).open():
  a=json.loads(line)['a'];values[json.dumps(a,sort_keys=True)]=a
pool=list(values.values());none=dict(kind='none');commands=[]
for _ in range(5000):
 left=r.sample(pool,r.randrange(5));right=r.sample(pool,r.randrange(5));commands.append(dict(op='meet_lists',a=none,left=left,right=right))
 pairs=[[r.choice(pool),r.choice(pool)]for _ in range(r.randrange(5))];commands.append(dict(op='map_pairs',a=none,pairs=pairs))
commands += [dict(op='substitute_all_variables',a=a)for a in r.sample(pool,3000)]
commands += [dict(op='function_arity',a=none,arity=arity,range=t)for arity in [-1,0,1,2,254,255,256]for t in r.sample(pool,100)]
commands += [dict(op='atoms_constructor',a=none,values=['a'+str(n)for n in range(size)])for size in [0,1,2,13,14,20]]
p=root/'list_api_commands.jsonl';out=root/'list_api_results.jsonl';p.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with p.open('rb')as src,out.open('wb')as dest:subprocess.run([str(WORKSPACE/'target/release/type_probe')],stdin=src,stdout=dest,check=True)
subprocess.run(['elixir',str(WORKSPACE/'scripts/type_oracle.exs'),str(p),str(out)],check=True)
