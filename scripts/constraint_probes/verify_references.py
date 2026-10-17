import json,subprocess,os,random
from pathlib import Path
from config import configure
config=configure();root=Path(__file__).resolve().parents[2];h=config.out;h.mkdir(parents=True,exist_ok=True);rng=random.Random(867)
a=lambda k:{'kind':k};atom=lambda *v:{'kind':'atoms','values':list(v)};integer=lambda *v:{'kind':'integers','values':[str(x)for x in v]};key=lambda n:{'integer':str(n)};var=lambda n:{'kind':'var','id':key(n)};fun=lambda domain,ret:{'kind':'function','domain':domain,'range':ret}
pool=[a('any'),a('none'),a('unit'),a('atom'),a('integer'),atom('a'),atom('z'),integer(0),integer(1)]
commands=[]
def refs(nodes,body,recursive=False,prop=None,rec=None):
 def vars(x):
  if isinstance(x,dict):
   if x.get('kind')=='var':return{int(x['id']['integer'])}
   return set().union(*(vars(v)for v in x.values()))
  if isinstance(x,list):return set().union(*(vars(v)for v in x))
  return set()
 deps=sorted(vars(nodes[:body+1]));nodes.append({'kind':'reference','function':key(100),'dependencies':[key(v)for v in deps]})
 return{'tree':nodes,'root':len(nodes)-1,'functions':[{'id':key(100),'body':body,'arity':1,'recursive_variable':key(rec)if rec else None,'self_recursive':recursive,'propagated_domain':prop}],'bindings':[]}
for i in range(1000):
 nodes=[{'kind':'atomic','left':var(100),'right':fun([var(1)],var(2)),'relation':'eq'}]
 for _ in range(rng.randrange(1,6)):nodes.append({'kind':'atomic','left':rng.choice([var(1),var(2)]),'right':rng.choice(pool),'relation':rng.choice(['eq','sub'])})
 nodes.append({'kind':'conj','children':list(range(len(nodes)))})
 c=refs(nodes,len(nodes)-1,prop=None if i%2 else [rng.choice(pool)],rec=77 if i%3==0 else None)
 if i%4==0:c['bindings']=[{'id':key(1),'type':rng.choice(pool)}]
 commands.append(c)
for i in range(400):
 nodes=[]
 def atomc(left,right):nodes.append({'kind':'atomic','left':left,'right':right,'relation':'eq'});return len(nodes)-1
 def scope(kind,children):nodes.append({'kind':kind,'children':children});return len(nodes)-1
 proto=scope('conj',[atomc(var(100),fun([var(1)],var(2)))])
 base=scope('conj',[atomc(var(1),rng.choice(pool[3:])),atomc(var(2),rng.choice(pool[3:]))])
 recur=scope('conj',[atomc(var(77),fun([var(1)],var(2)))])
 disj=scope('disj',[base,recur]);body=scope('conj',[proto,disj]);commands.append(refs(nodes,body,True,rec=77))
# A recursion with no base path exercises the native unit second pass.
nodes=[{'kind':'atomic','left':var(100),'right':fun([var(1)],var(2)),'relation':'eq'},{'kind':'atomic','left':var(77),'right':fun([var(1)],var(2)),'relation':'eq'},{'kind':'conj','children':[0,1]}]
commands.append(refs(nodes,2,True,rec=77))
# Repeated references share a function-local cache; outer changes invalidate its body.
for i in range(200):
 nodes=[{'kind':'atomic','left':var(100),'right':fun([var(1)],var(2)),'relation':'eq'},{'kind':'atomic','left':var(2),'right':rng.choice(pool),'relation':'eq'},{'kind':'conj','children':[0,1]}]
 c=refs(nodes,2,False,rec=77 if i%2 else None);first=c['root']
 nodes.append({'kind':'atomic','left':var(1),'right':rng.choice(pool),'relation':'eq'});nodes.append({'kind':'conj','children':[len(nodes)-1]});outer=len(nodes)-1
 nodes.append(dict(nodes[first]));second=len(nodes)-1
 nodes.append({'kind':'conj','children':[first,outer,second]});c['root']=len(nodes)-1;commands.append(c)
# Raw recursive record keys can already exist in the caller map; the native
# reference assertion preserves badmatch:error when their intersection fails.
for ty in pool:
 nodes=[{'kind':'atomic','left':var(100),'right':fun([var(1)],var(2)),'relation':'eq'},{'kind':'conj','children':[0]}]
 c=refs(nodes,1,False,rec=77)
 c['bindings']=[{'id':{'raw_variable_record':key(77)},'type':ty}]
 commands.append(c)
input_=h/'references.jsonl';input_.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with input_.open('rb')as inp:rs=subprocess.check_output([str(config.binary)],stdin=inp)
(h/'references.rust.jsonl').write_bytes(rs);rust=[json.loads(x)for x in rs.splitlines()]
env=config.env;subprocess.run(['elixir',str(root/'scripts/constraint_probes/native_tree.exs'),str(root),str(input_),str(h/'references.native.json')],env=env,check=True)
native=json.loads((h/'references.native.json').read_text());assert len(rust)==len(native)==len(commands),'incomplete oracle or Rust output';failures=[{'index':i,'command':commands[i],'rust':r,'native':n}for i,(r,n)in enumerate(zip(rust,native))if {k:v for k,v in r.items()if k!='detail'}!={k:v for k,v in n.items()if k!='detail'}];(h/'references-verification.json').write_text(json.dumps({'commands':len(commands),'passed':len(commands)-len(failures),'failures':failures},indent=2));print(len(commands),len(failures),flush=True)
