import json,subprocess,os,itertools
from pathlib import Path
from config import configure
config=configure();root=Path(__file__).resolve().parents[2];h=config.out;h.mkdir(parents=True,exist_ok=True)
a=lambda k:{'kind':k};atom=lambda *v:{'kind':'atoms','values':list(v)};integer=lambda *v:{'kind':'integers','values':[str(x)for x in v]};var=lambda v:{'kind':'var','id':{'atom':v}if isinstance(v,str)else{'integer':str(v)}};tuple_=lambda *v:{'kind':'tuple','elements':list(v)};product=lambda *v:{'kind':'product','values':list(v)};fun=lambda args,r:{'kind':'function','domain':args,'range':r};list_=lambda t:{'kind':'list','elements':t,'tail':a('nil'),'nonempty':False}
pool=[a(k)for k in ['any','none','unit','atom','integer','number','float','nil','binary','tuple_any','map_any']]+[atom('a'),atom('a','z'),integer(0),integer(-1,0,1),{'kind':'range','from':'0','to':'10'},var('x'),var('y'),var(1),tuple_(var('x'),a('integer')),tuple_(atom('tag'),var('x')),tuple_(atom('tag'),integer(1)),list_(var('x')),list_(a('integer')),product(var('x'),var('y')),product(integer(1),atom('a')),fun([var('x')],var('y')),fun([a('integer')],atom('a')),fun(None,a('any'))]
nom=lambda name,inner:{'kind':'nominal','name':{'module':'m','name':name,'arity':0,'opacity':'opaque'},'inner':inner}
pool += [nom('n',a('any')),nom('n',a('integer')),nom('n',var('x')),{'kind':'nominal_set','nominals':[nom('n',var('x')),nom('o',a('atom'))],'structural':var('y')}]
symbol_nom=lambda name,inner:{'kind':'nominal','name':{'symbol':name},'inner':inner}
symbolic_set={'kind':'nominal_set','nominals':[symbol_nom('a',var(1)),symbol_nom('b',var(2))],'structural':a('none')}
deep=atom('deep')
for _ in range(8):deep=tuple_(deep)
pool += [symbolic_set,symbol_nom('a',atom('ok')),dict(symbolic_set,structural=deep),{'kind':'nominal_set','nominals':[symbol_nom('a',atom('ok'))],'structural':deep}]
for depth in [1,5,8]:
 t=var('x')
 for _ in range(depth):t=tuple_(t)
 pool.append(t)
scopes=[[],[{'id':{'atom':'x'},'type':a('integer')}],[{'id':{'atom':'x'},'type':atom('a')},{'id':{'atom':'y'},'type':a('integer')}],[{'id':{'atom':'x'},'type':a('any')},{'id':{'integer':'1'},'type':integer(1)}]]
commands=[{'left':l,'right':r,'relation':op,'bindings':scope}for l,r,op,scope in itertools.product(pool,pool,['eq','sub'],scopes)]
input_=h/'atomic.jsonl';input_.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with input_.open('rb')as inp:rs=subprocess.check_output([str(config.binary)],stdin=inp)
(h/'atomic.rust.jsonl').write_bytes(rs);rust=[json.loads(x)for x in rs.splitlines()]
env=config.env;subprocess.run(['elixir',str(root/'scripts/constraint_probes/native_atomic.exs'),str(root),str(input_),str(h/'atomic.native.json')],env=env,check=True)
native=json.loads((h/'atomic.native.json').read_text());assert len(rust)==len(native)==len(commands),'incomplete oracle or Rust output';failures=[]
for i,(r,n)in enumerate(zip(rust,native)):
 if {k:v for k,v in r.items()if k!='detail'}!={k:v for k,v in n.items()if k!='detail'}:failures.append({'index':i,'command':commands[i],'rust':r,'native':n})
(h/'atomic-verification.json').write_text(json.dumps({'commands':len(commands),'passed':len(commands)-len(failures),'failures':failures},indent=2));print(len(commands),len(failures),flush=True)
