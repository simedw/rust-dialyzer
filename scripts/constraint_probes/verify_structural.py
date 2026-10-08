import json,random,os,subprocess,itertools
from pathlib import Path
root=Path(__file__).resolve().parents[2];h=root/'.cache/constraints';h.mkdir(parents=True,exist_ok=True);rng=random.Random(271)
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
commands=[]
for operation in ['cons','cons_head','cons_tail','binary_concat','binary_match']:
 for _ in range(1000):
  d={'operation':operation}
  if operation=='cons':d.update(head=var(1),tail=var(2))
  elif operation.startswith('cons_'):d.update(value=var(1))
  else:d.update(left=var(1),right=var(2))
  commands.append({'derived':d,'bindings':[{'id':{'integer':str(i)},'type':rng.choice(pool)}for i in [1,2]]})
size_pool=pool+[integer(0),integer(1),integer(127),integer(128),integer(1000),{'kind':'range','from':'0','to':'7'},{'kind':'range','from':'1','to':'128'}]
for operation in ['binary_length','binary_value']:
 for unit in [1,2,8,16,256]:
  for mode in (['construct','match']if operation=='binary_length'else[False,True]):
   for size in size_pool:
    d={'operation':operation,'size':var(1),'unit':unit}
    d.update({'mode':mode}if operation=='binary_length'else{'signed':mode})
    commands.append({'derived':d,'bindings':[{'id':key(1),'type':size},{'id':key(2),'type':a('any')}]})
for op in ['append_head','append_tail']:
 for value in pool:commands.append({'derived':{'operation':op,'destination':var(1)},'bindings':[{'id':key(1),'type':value},{'id':key(2),'type':a('any')}]})
for _ in range(1000):
 value,keytype,shadow=[rng.choice(pool)for i in range(3)]
 entries=[{'key':rng.choice(pool),'value':rng.choice(pool),'operation':rng.choice(['associate','exact'])}for i in range(rng.randrange(5))]
 for op in ['map_construct','map_argument','map_value']:
  d={'operation':op,'entries':entries,'argument':var(1),'key':keytype,'map':var(1),'shadow_keys':[shadow]}
  commands.append({'derived':d,'bindings':[{'id':key(1),'type':value},{'id':key(2),'type':a('any')}]})
for _ in range(250):commands.append({'derived':{'operation':'binary_segments','segments':[rng.choice(pool)for i in range(rng.randrange(6))]},'bindings':[{'id':key(1),'type':a('any')},{'id':key(2),'type':a('any')}]})
input_=h/'structural.jsonl';input_.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with input_.open('rb')as inp:rs=subprocess.check_output([str(root/'target/release/constraints_probe')],stdin=inp)
(h/'structural.rust.jsonl').write_bytes(rs);rust=[json.loads(x)for x in rs.splitlines()]
env=os.environ.copy();env.update(ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28');subprocess.run(['elixir',str(root/'scripts/constraint_probes/native_structural.exs'),str(root),str(input_),str(h/'structural.native.json')],env=env,check=True)
native=json.loads((h/'structural.native.json').read_text());assert len(rust)==len(native)==len(commands),'incomplete oracle or Rust output'
failures=[{'index':i,'command':commands[i],'rust':r,'native':n}for i,(r,n)in enumerate(zip(rust,native))if {k:v for k,v in r.items()if k!='detail'}!={k:v for k,v in n.items()if k!='detail'}];(h/'structural-verification.json').write_text(json.dumps({'commands':len(commands),'passed':len(commands)-len(failures),'failures':failures},indent=2));print(len(commands),len(failures),flush=True)
