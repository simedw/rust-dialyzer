import json,random,itertools,subprocess
from pathlib import Path
WORKSPACE=Path(__file__).resolve().parents[2];root=WORKSPACE/".cache/types";r=random.Random(28313)
values={}
for file in ['map_commands.jsonl','nominal_commands.jsonl','raw_commands.jsonl']:
 for line in (root/file).open():
  a=json.loads(line)['a'];values[json.dumps(a,sort_keys=True)]=a
pool=list(values.values());commands=[dict(op='map_pairwise_merge',mode=mode,a=r.choice(pool),b=r.choice(pool))for _ in range(3000)for mode in ['skip','left','right','join']]
maps=[a for a in pool if a['kind'] in ['map','map_any']]
commands += [dict(op='map_pairwise_merge',mode=mode,a=r.choice(maps),b=r.choice(maps))for _ in range(4000)for mode in ['skip','left','right','join']]
endpoints=['neg_inf','pos_inf']+[str(n)for n in [-1,0,1,-(1<<180),1<<180]]
commands += [dict(op=op,a=dict(kind='none'),left=x,right=y)for op,x,y in itertools.product(['endpoint_min','endpoint_max'],endpoints,endpoints)]
p=root/'merge_api_commands.jsonl';out=root/'merge_api_results.jsonl';p.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with p.open('rb')as src,out.open('wb')as dest:subprocess.run([str(WORKSPACE/'target/release/type_probe')],stdin=src,stdout=dest,check=True)
subprocess.run(['elixir',str(WORKSPACE/'scripts/type_oracle.exs'),str(p),str(out)],check=True)
