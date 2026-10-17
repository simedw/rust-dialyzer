import json,subprocess,os,random
from pathlib import Path
from config import configure
config=configure();root=Path(__file__).resolve().parents[2];h=config.out;h.mkdir(parents=True,exist_ok=True);rng=random.Random(479)
a=lambda k:{'kind':k};atom=lambda *v:{'kind':'atoms','values':list(v)};integer=lambda *v:{'kind':'integers','values':[str(x)for x in v]};key=lambda n:{'integer':str(n)};var=lambda n:{'kind':'var','id':key(n)}
pool=[a('any'),a('none'),a('unit'),a('atom'),a('integer'),a('number'),a('float'),atom('a'),atom('b'),atom('true'),integer(-1),integer(0),integer(1),var(1),var(2),var(-3),{'kind':'tuple','elements':[atom('tag'),var(1)]},{'kind':'function','domain':[var(1)],'range':var(2)},{'kind':'list','elements':var(1),'tail':a('nil'),'nonempty':False}]
for opacity in ['transparent','opaque']:
 for n,inner in enumerate(pool[:13]):pool.append({'kind':'nominal','name':{'module':'owner','name':'n'+str(n),'arity':0,'opacity':opacity},'inner':inner})
commands=[]
for _ in range(2000):
 nodes=[]
 for i in range(rng.randrange(4,20)):
  if i>1 and rng.random()<.55:
   nodes.append({'kind':rng.choice(['conj','disj']),'children':[rng.randrange(i)for _ in range(rng.randrange(0,6))]})
  elif rng.random()<.13:
   function=rng.choice([1,2,100]);deps=sorted(rng.sample([-3,1,2,100],rng.randrange(0,5)));nodes.append({'kind':'ref','function':key(function),'deps':[key(n)for n in deps]})
  else:nodes.append({'kind':'atomic','left':rng.choice(pool),'right':rng.choice(pool),'relation':rng.choice(['eq','sub'])})
 commands.append({'normalize':{'nodes':nodes,'root':len(nodes)-1}})
for c in commands:
 if c['normalize']['nodes'][-1]['kind']in ['conj','disj']:c['normalize']['dnf']=bool(rng.randrange(2))
# Nested one-element disjunction and tautologies follow different constructor paths.
commands += [{'normalize':{'nodes':[{'kind':'atomic','left':var(1),'right':atom('a'),'relation':'eq'},{'kind':'disj','children':[0]}],'root':1}}, {'normalize':{'nodes':[{'kind':'conj','children':[]},{'kind':'disj','children':[0]}],'root':1}}]
def product_case(a_count,b_count):
 nodes=[];groups=[]
 for variable,count in [(1,a_count),(2,b_count)]:
  leaves=[]
  for n in range(count):leaves.append(len(nodes));nodes.append({'kind':'atomic','left':var(variable),'right':atom('a'+str(n)),'relation':'eq'})
  groups.append(len(nodes));nodes.append({'kind':'disj','children':leaves})
 nodes.append({'kind':'conj','children':groups});return {'normalize':{'nodes':nodes,'root':len(nodes)-1,'dnf':True}}
commands += [product_case(9,3),product_case(7,4),product_case(10,3),product_case(29,2)]
nodes=[{'kind':'atomic','left':var(1),'right':atom('first'),'relation':'sub'}];prev=0
for n in range(300):
 leaf=len(nodes);nodes.append({'kind':'atomic','left':var(1),'right':atom('x%04d'%n),'relation':'sub'})
 nodes.append({'kind':'disj','children':[prev,leaf]});branch=len(nodes)-1
 nodes.append({'kind':'conj','children':[branch,leaf]});prev=len(nodes)-1
commands.append({'normalize':{'nodes':nodes,'root':prev}})
inp=h/'normalize.jsonl';inp.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with inp.open('rb')as f:result=subprocess.check_output([str(config.binary)],stdin=f)
rust=[json.loads(x)for x in result.splitlines()];(h/'normalize.rust.jsonl').write_bytes(result)
env=config.env;subprocess.run(['elixir',str(root/'scripts/constraint_probes/native_normalize.exs'),str(root),str(inp),str(h/'normalize.native.json')],env=env,check=True)
native=json.loads((h/'normalize.native.json').read_text());assert len(rust)==len(native)==len(commands)==2007
failures=[{'index':i,'command':commands[i],'rust':r,'native':n}for i,(r,n)in enumerate(zip(rust,native))if{k:v for k,v in r.items()if k!='detail'}!={k:v for k,v in n.items()if k!='detail'}]
(h/'normalize-verification.json').write_text(json.dumps({'commands':len(commands),'passed':len(commands)-len(failures),'failures':failures},indent=2));print(len(commands),len(failures))
