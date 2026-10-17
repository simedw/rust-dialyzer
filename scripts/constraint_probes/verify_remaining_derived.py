import json,random,os,subprocess,itertools
from pathlib import Path
from config import configure
config=configure();root=Path(__file__).resolve().parents[2];h=config.out;h.mkdir(parents=True,exist_ok=True);rng=random.Random(271)
a=lambda k:{'kind':k};atom=lambda *v:{'kind':'atoms','values':list(v)};integer=lambda *v:{'kind':'integers','values':[str(x)for x in v]};key=lambda n:{'integer':str(n)};var=lambda n:{'kind':'var','id':key(n)}
pool=[a('any'),a('none'),a('unit'),a('atom'),a('integer'),a('number'),a('float'),atom('a'),atom('true'),atom('false'),atom('true','false'),integer(-1),integer(0),integer(1),integer(2),{'kind':'range','from':'0','to':'pos_inf'},{'kind':'range','from':'neg_inf','to':'-1'},{'kind':'range','from':'-2','to':'3'}]
for opacity in ['transparent','opaque']:
 for n,inner in [('integer',a('integer')),('number',a('number')),('float',a('float')),('true',atom('true')),('boolean',atom('true','false')),('any',a('any')),('negative',{'kind':'range','from':'neg_inf','to':'-1'})]:
  pool.append({'kind':'nominal','name':{'module':'typed_owner','name':n,'arity':0,'opacity':opacity},'inner':inner})
structural=[a('nil'),a('tuple_any'),{'kind':'map','entries':[],'default_key':a('any'),'default_value':a('any')},{'kind':'tuple','elements':[atom('tag'),a('integer')]},{'kind':'list','elements':a('integer'),'tail':a('nil'),'nonempty':False},{'kind':'list','elements':atom('a'),'tail':a('nil'),'nonempty':True},{'kind':'list','elements':a('atom'),'tail':a('any'),'nonempty':True},{'kind':'list','elements':a('integer'),'tail':atom('tail'),'nonempty':True}]
structural += [{'kind':'bitstring','unit':str(u),'base':str(b)}for u,b in [(0,0),(0,7),(0,16),(1,0),(2,3),(8,0),(8,16),(16,8)]]
pool+=structural
for opacity in ['transparent','opaque']:
 for i,inner in enumerate(structural):pool.append({'kind':'nominal','name':{'module':'typed_owner','name':'struct'+str(i),'arity':0,'opacity':opacity},'inner':inner})
binary=config.binary
assert config.binary_explicit or binary.stat().st_mtime >= max(p.stat().st_mtime for p in (root/'src').rglob('*.rs') if p.name != 'tests.rs'), 'Rust probe stale'
commands=[]
arity_pool=pool+[integer(-1),integer(0),integer(1),integer(254),integer(255),integer(256),integer(10000000000000000000000000000000000000)]
for _ in range(2000):
 commands.append({'derived':{'operation':'function_arity_argument','destination':var(1),'arity':var(2)},'bindings':[{'id':key(1),'type':rng.choice(pool)},{'id':key(2),'type':rng.choice(arity_pool)}]})
for destination in [atom('true'),atom('false'),a('none')]:
 for arity in [-1,0,1,254,255,256]:commands.append({'derived':{'operation':'function_arity_argument','destination':var(1),'arity':var(2)},'bindings':[{'id':key(1),'type':destination},{'id':key(2),'type':integer(arity)}]})
for _ in range(1000):
 commands.append({'derived':{'operation':'clause_residual','argument':var(1),'subtract':[rng.choice(pool)for _ in range(rng.randrange(5))]},'bindings':[{'id':key(1),'type':rng.choice(pool)},{'id':key(2),'type':a('any')}]})
input_=h/'remaining-derived.jsonl';input_.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with input_.open('rb')as inp:rs=subprocess.check_output([str(binary)],stdin=inp)
(h/'remaining-derived.rust.jsonl').write_bytes(rs);rust=[json.loads(x)for x in rs.splitlines()]
env=config.env;subprocess.run(['elixir',str(root/'scripts/constraint_probes/native_structural.exs'),str(root),str(input_),str(h/'remaining-derived.native.json')],env=env,check=True)
native=json.loads((h/'remaining-derived.native.json').read_text());assert len(rust)==len(native)==len(commands)==3018,'incomplete oracle or Rust output'
failures=[{'index':i,'command':commands[i],'rust':r,'native':n}for i,(r,n)in enumerate(zip(rust,native))if {k:v for k,v in r.items()if k!='detail'}!={k:v for k,v in n.items()if k!='detail'}];(h/'remaining-derived-verification.json').write_text(json.dumps({'commands':len(commands),'passed':len(commands)-len(failures),'failures':failures},indent=2));print(len(commands),len(failures),flush=True)
