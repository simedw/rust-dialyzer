import json,random,os,subprocess
from pathlib import Path
from config import configure
config=configure();root=Path(__file__).resolve().parents[2];h=config.out;h.mkdir(parents=True,exist_ok=True);rng=random.Random(137)
a=lambda k:{'kind':k};atom=lambda *v:{'kind':'atoms','values':list(v)};integer=lambda *v:{'kind':'integers','values':[str(x)for x in v]};key=lambda n:{'integer':str(n)};var=lambda n:{'kind':'var','id':key(n)}
pool=[a('any'),a('none'),a('unit'),a('atom'),a('integer'),a('number'),a('float'),atom('a'),atom('true'),atom('false'),atom('true','false'),integer(-1),integer(0),integer(1),integer(2),{'kind':'range','from':'0','to':'pos_inf'},{'kind':'range','from':'neg_inf','to':'-1'},{'kind':'range','from':'-2','to':'3'}]
commands=[]
for source in ['+','-','*','<','=<','>','>=','and','or','=:=','==','is_atom','is_integer','is_boolean','not']:
 for _ in range(150):
  ops=[]
  unary=source in ['is_atom','is_integer','is_boolean','not'];arity=1 if unary else 2
  def op(d,lhs):d.update(native_mfa={'module':'erlang','function':source,'arity':arity},native_lhs=lhs);ops.append(d)
  if source in ['+','-','*']:
   op({'operation':'bif','mfa':{'module':'erlang','function':source,'arity':2},'arguments':[var(1),var(2)]},100)
   for lhs,other,pos in [(1,2,2),(2,1,1)]:op({'operation':'inverse','arithmetic':source,'known_position':pos,'destination':var(100),'other':var(other)},lhs)
  elif source in ['<','=<','>','>=']:
   op({'operation':'bif','mfa':{'module':'erlang','function':source,'arity':2},'arguments':[var(1),var(2)]},100)
   local2={'<':'>=','=<':'>=','>':'<','>=':'=<'}[source]
   for lhs,other,local in [(1,2,source),(2,1,local2)]:op({'operation':'ordering','ordering':local,'destination':var(100),'argument':var(lhs),'other':var(other)},lhs)
  elif source in ['and','or']:
   op({'operation':'boolean_return','boolean':source,'left':var(1),'right':var(2)},100)
   for lhs,other in [(1,2),(2,1)]:op({'operation':'boolean_argument','boolean':source,'destination':var(100),'other':var(other)},lhs)
  elif source in ['=:=','==']:
   op({'operation':'exact_equality_return','left':var(1),'right':var(2)}if source=='=:='else{'operation':'bif','mfa':{'module':'erlang','function':source,'arity':2},'arguments':[var(1),var(2)]},100)
   for lhs,other in [(1,2),(2,1)]:op({'operation':'equality_argument','equality':source,'destination':var(100),'argument':var(lhs),'other':var(other)},lhs)
  elif source=='not':
   op({'operation':'not','value':var(1)},100);op({'operation':'not','value':var(100)},1)
  else:
   test={'is_atom':a('atom'),'is_integer':a('integer'),'is_boolean':atom('true','false')}[source]
   op({'operation':'predicate_return','argument':var(1),'test':test},100);op({'operation':'predicate_argument','destination':var(100),'test':test},1)
  nodes=[{'kind':'atomic','left':var(d['native_lhs']),'right':{'derived':i},'relation':'sub'}for i,d in enumerate(ops)]
  children=list(range(len(nodes)));nodes.append({'kind':'conj','children':children});commands.append({'derived_nodes':ops,'tree':nodes,'root':len(nodes)-1,'bindings':[{'id':key(k),'type':rng.choice(pool)}for k in ([1,100]if unary else[1,2,100])]})
# Equality against a raw fun_var fails native unification for an ordinary computed type.
commands.append({'derived_nodes':[{'operation':'predicate_argument','destination':var(100),'test':a('integer'),'native_mfa':{'module':'erlang','function':'is_integer','arity':1},'native_lhs':1}],'tree':[{'kind':'atomic','left':var(1),'right':{'derived':0},'relation':'eq'}],'root':0,'bindings':[]})
input_=h/'dynamic.jsonl';input_.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with input_.open('rb')as inp:rs=subprocess.check_output([str(config.binary)],stdin=inp)
(h/'dynamic.rust.jsonl').write_bytes(rs);rust=[json.loads(x)for x in rs.splitlines()]
env=config.env;subprocess.run(['elixir',str(root/'scripts/constraint_probes/native_tree.exs'),str(root),str(input_),str(h/'dynamic.native.json')],env=env,check=True)
native=json.loads((h/'dynamic.native.json').read_text());assert len(rust)==len(native)==len(commands),'incomplete oracle or Rust output'
failures=[{'index':i,'command':commands[i],'rust':r,'native':n}for i,(r,n)in enumerate(zip(rust,native))if {k:v for k,v in r.items()if k!='detail'}!={k:v for k,v in n.items()if k!='detail'}];(h/'dynamic-verification.json').write_text(json.dumps({'commands':len(commands),'passed':len(commands)-len(failures),'failures':failures},indent=2));print(len(commands),len(failures),flush=True)
