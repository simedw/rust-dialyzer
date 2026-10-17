import json,random,os,subprocess,itertools
from pathlib import Path
from config import configure
config=configure();root=Path(__file__).resolve().parents[2];h=config.out;h.mkdir(parents=True,exist_ok=True);rng=random.Random(271)
a=lambda k:{'kind':k};atom=lambda *v:{'kind':'atoms','values':list(v)};integer=lambda *v:{'kind':'integers','values':[str(x)for x in v]};key=lambda n:{'integer':str(n)};var=lambda n:{'kind':'var','id':key(n)}
pool=[a('any'),a('none'),a('unit'),a('atom'),a('integer'),a('number'),a('float'),atom('a'),atom('true'),atom('false'),atom('true','false'),integer(-1),integer(0),integer(1),integer(2),{'kind':'range','from':'0','to':'pos_inf'},{'kind':'range','from':'neg_inf','to':'-1'},{'kind':'range','from':'-2','to':'3'}]
for opacity in ['transparent','opaque']:
 for n,inner in [('integer',a('integer')),('number',a('number')),('float',a('float')),('true',atom('true')),('boolean',atom('true','false')),('any',a('any')),('negative',{'kind':'range','from':'neg_inf','to':'-1'})]:
  pool.append({'kind':'nominal','name':{'module':'typed_owner','name':n,'arity':0,'opacity':opacity},'inner':inner})
commands=[]
def add(d,mfa,lhs,types):commands.append({'derived':d,'bindings':[{'id':key(k),'type':ty}for k,ty in types.items()],'native_mfa':{'module':'erlang','function':mfa,'arity':2 if mfa in ['+','-','*','<','=<','>','>=','and','or','=:=','==']else 1},'native_lhs':lhs})
for arithmetic in ['+','-','*']:
 for lhs,position,other in [(1,2,2),(2,1,1)]:
  for dst,known in itertools.product(pool,pool):add({'operation':'inverse','arithmetic':arithmetic,'known_position':position,'destination':var(100),'other':var(other)},arithmetic,lhs,{100:dst,other:known})
for source,local2 in [('<','>='),('=<','>='),('>','<'),('>=','=<')]:
 for lhs,other,local in [(1,2,source),(2,1,local2)]:
  for _ in range(1000):
   dst,arg,known=[rng.choice(pool)for _ in range(3)]
   add({'operation':'ordering','ordering':local,'destination':var(100),'argument':var(lhs),'other':var(other)},source,lhs,{100:dst,lhs:arg,other:known})
for name,test in [('is_atom',a('atom')),('is_integer',a('integer')),('is_number',a('number')),('is_float',a('float')),('is_boolean',atom('true','false'))]:
 for ty in pool:
  add({'operation':'predicate_argument','destination':var(100),'test':test},name,1,{100:ty})
  add({'operation':'predicate_return','argument':var(1),'test':test},name,100,{1:ty})
for boolean in ['and','or']:
 for lhs,other in [(1,2),(2,1)]:
  for _ in range(500):
   dst,known=[rng.choice(pool)for _ in range(2)]
   add({'operation':'boolean_argument','boolean':boolean,'destination':var(100),'other':var(other)},boolean,lhs,{100:dst,other:known})
 for _ in range(500):
  left,right=[rng.choice(pool)for _ in range(2)]
  add({'operation':'boolean_return','boolean':boolean,'left':var(1),'right':var(2)},boolean,100,{1:left,2:right})
for ty in pool:
 add({'operation':'not','value':var(100)},'not',1,{100:ty})
 add({'operation':'not','value':var(1)},'not',100,{1:ty})
for equality in ['=:=','==']:
 for lhs,other in [(1,2),(2,1)]:
  for _ in range(1000):
   dst,arg,known=[rng.choice(pool)for _ in range(3)]
   add({'operation':'equality_argument','equality':equality,'destination':var(100),'argument':var(lhs),'other':var(other)},equality,lhs,{100:dst,lhs:arg,other:known})
for _ in range(1000):
 left,right=[rng.choice(pool)for _ in range(2)]
 add({'operation':'exact_equality_return','left':var(1),'right':var(2)},'=:=',100,{1:left,2:right})
input_=h/'derived.jsonl';input_.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with input_.open('rb')as inp:rs=subprocess.check_output([str(config.binary)],stdin=inp)
(h/'derived.rust.jsonl').write_bytes(rs);rust=[json.loads(x)for x in rs.splitlines()]
env=config.env;subprocess.run(['elixir',str(root/'scripts/constraint_probes/native_derived.exs'),str(root),str(input_),str(h/'derived.native.json')],env=env,check=True)
native=json.loads((h/'derived.native.json').read_text());assert len(rust)==len(native)==len(commands),'incomplete oracle or Rust output'
failures=[{'index':i,'command':commands[i],'rust':r,'native':n}for i,(r,n)in enumerate(zip(rust,native))if {k:v for k,v in r.items()if k!='detail'}!={k:v for k,v in n.items()if k!='detail'}];(h/'derived-verification.json').write_text(json.dumps({'commands':len(commands),'passed':len(commands)-len(failures),'failures':failures},indent=2));print(len(commands),len(failures),flush=True)
