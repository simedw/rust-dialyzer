import json,random,os,subprocess
from pathlib import Path
from config import configure
config=configure();root=Path(__file__).resolve().parents[2];h=config.out;h.mkdir(parents=True,exist_ok=True);rng=random.Random(908)
a=lambda k:{'kind':k};atom=lambda *v:{'kind':'atoms','values':list(v)};integer=lambda *v:{'kind':'integers','values':[str(x)for x in v]};key=lambda n:{'integer':str(n)};var=lambda n:{'kind':'var','id':key(n)};fun=lambda domain,ret:{'kind':'function','domain':domain,'range':ret}
pool=[a('atom'),a('integer'),atom('a'),atom('z'),integer(0),integer(1)]
commands=[]
for i in range(401):
 nodes=[];functions=[];refs=[]
 def atomic(l,r):nodes.append({'kind':'atomic','left':l,'right':r,'relation':'eq'});return len(nodes)-1
 def scope(kind,children):nodes.append({'kind':kind,'children':children});return len(nodes)-1
 for fid,x,ret,rec,other in [(100,1,2,77,88),(200,3,4,88,77)]:
  proto=scope('conj',[atomic(var(fid),fun([var(x)],var(ret)))])
  recurrence=scope('conj',[atomic(var(other),fun([var(x)],var(ret)))])
  if i==400:branches=[recurrence]
  else:branches=[scope('conj',[atomic(var(x),rng.choice(pool)),atomic(var(ret),rng.choice(pool))]),recurrence]
  disj=scope('disj',branches);body=scope('conj',[proto,disj])
  deps=[x,ret,other,fid]
  nodes.append({'kind':'reference','function':key(fid),'dependencies':[key(v)for v in sorted(deps)]});refs.append(len(nodes)-1)
  functions.append({'id':key(fid),'body':body,'arity':1,'recursive_variable':key(rec),'self_recursive':False,'propagated_domain':None})
 root_id=scope('conj',refs);commands.append({'tree':nodes,'root':root_id,'functions':functions,'bindings':[],'scc':[key(100),key(200)]})
input_=h/'scc.jsonl';input_.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with input_.open('rb')as inp:rs=subprocess.check_output([str(config.binary)],stdin=inp)
(h/'scc.rust.jsonl').write_bytes(rs);rust=[json.loads(x)for x in rs.splitlines()]
env=config.env;subprocess.run(['elixir',str(root/'scripts/constraint_probes/native_tree.exs'),str(root),str(input_),str(h/'scc.native.json')],env=env,check=True)
native=json.loads((h/'scc.native.json').read_text());assert len(rust)==len(native)==len(commands),'incomplete oracle or Rust output';failures=[{'index':i,'command':commands[i],'rust':r,'native':n}for i,(r,n)in enumerate(zip(rust,native))if {k:v for k,v in r.items()if k!='detail'}!={k:v for k,v in n.items()if k!='detail'}];(h/'scc-verification.json').write_text(json.dumps({'commands':len(commands),'passed':len(commands)-len(failures),'failures':failures},indent=2));print(len(commands),len(failures),flush=True)
