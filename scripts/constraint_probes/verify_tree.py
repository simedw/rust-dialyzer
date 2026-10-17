import json,subprocess,os,itertools,random
from pathlib import Path
from config import configure
config=configure();root=Path(__file__).resolve().parents[2];h=config.out;h.mkdir(parents=True,exist_ok=True);rng=random.Random(713)
a=lambda k:{'kind':k};atom=lambda *v:{'kind':'atoms','values':list(v)};integer=lambda *v:{'kind':'integers','values':[str(x)for x in v]};var=lambda v:{'kind':'var','id':{'integer':str({'x':1,'y':2,'z':3}[v])}}
pool=[a('any'),a('none'),a('unit'),a('atom'),a('integer'),atom('a'),atom('z'),integer(0),integer(1),var('x'),var('y'),var('z')]
commands=[]
for i in range(2000):
 nodes=[]
 def atomic():
  n={'kind':'atomic','left':rng.choice(pool),'right':rng.choice(pool),'relation':rng.choice(['eq','sub'])};nodes.append(n);return len(nodes)-1
 def scope(kind,children):nodes.append({'kind':kind,'children':children});return len(nodes)-1
 branches=[scope('conj',[atomic()for _ in range(rng.randrange(1,5))])for _ in range(rng.randrange(1,5))]
 disj=scope('disj',branches)
 if i%2:flat=scope('conj',[atomic()for _ in range(rng.randrange(1,4))]);root_id=scope('conj',[flat,disj])
 else:root_id=disj
 commands.append({'tree':nodes,'root':root_id,'bindings':[]if i%3 else[{'id':{'integer':'1'},'type':rng.choice(pool[:9])}]})
# Dirty disjunction cache must invalidate branch locals before restoring them.
nodes=[{'kind':'atomic','left':var('x'),'right':atom('a'),'relation':'eq'}, {'kind':'atomic','left':var('y'),'right':atom('first'),'relation':'eq'}, {'kind':'conj','children':[0,1]}, {'kind':'atomic','left':var('x'),'right':atom('z'),'relation':'eq'}, {'kind':'atomic','left':var('y'),'right':atom('second'),'relation':'eq'}, {'kind':'conj','children':[3,4]}, {'kind':'disj','children':[2,5]}, {'kind':'atomic','left':var('x'),'right':atom('z'),'relation':'eq'}, {'kind':'conj','children':[7]}, {'kind':'conj','children':[6,8]}]
commands.append({'tree':nodes,'root':9,'bindings':[]})
# A flat transport and explicit executor must withstand deep scope nesting.
nodes=[{'kind':'atomic','left':var('x'),'right':atom('a'),'relation':'eq'}]
for _ in range(10000):nodes.append({'kind':'conj','children':[len(nodes)-1]})
commands.append({'tree':nodes,'root':len(nodes)-1,'bindings':[]})
input_=h/'tree.jsonl';input_.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with input_.open('rb')as inp:rs=subprocess.check_output([str(config.binary)],stdin=inp)
(h/'tree.rust.jsonl').write_bytes(rs);rust=[json.loads(x)for x in rs.splitlines()]
env=config.env;subprocess.run(['elixir',str(root/'scripts/constraint_probes/native_tree.exs'),str(root),str(input_),str(h/'tree.native.json')],env=env,check=True)
native=json.loads((h/'tree.native.json').read_text());assert len(rust)==len(native)==len(commands),'incomplete oracle or Rust output';failures=[{'index':i,'command':commands[i],'rust':r,'native':n}for i,(r,n)in enumerate(zip(rust,native))if {k:v for k,v in r.items()if k!='detail'}!={k:v for k,v in n.items()if k!='detail'}];(h/'tree-verification.json').write_text(json.dumps({'commands':len(commands),'passed':len(commands)-len(failures),'failures':failures},indent=2));print(len(commands),len(failures),flush=True)
