import json,random,subprocess,struct,argparse
from pathlib import Path
from probe_config import configuration
WORKSPACE=Path(__file__).resolve().parents[2];CONFIG=configuration(WORKSPACE);root=CONFIG.out;r=random.Random(28315);commands=[]
def compact(terms,root):
 output=[];memo={}
 def visit(id):
  if id in memo:return memo[id]
  t=dict(terms[id])
  if t['kind']in ['tuple','list']:t['items']=[visit(i)for i in t['items']]
  if t['kind']=='list':t['tail']=visit(t['tail'])
  if t['kind']=='map':t['entries']=[[visit(k),visit(v)]for k,v in t['entries']]
  output.append(t);memo[id]=len(output)-1;return memo[id]
 return dict(root=visit(root),terms=output)
parser=argparse.ArgumentParser();parser.add_argument('--requests',default=str(WORKSPACE/'.cache/form_metadata/environment_forms.requests.jsonl'));args=parser.parse_args();requests=Path(args.requests)
for line in requests.open():
 request=json.loads(line);core=json.load(open(request['core']))
 for node in request['nodes']:
  commands.append(dict(op='form_to_string',a=dict(kind='none'),arena=compact(core['modules'][node['module']]['terms'],node['raw'])))
class Atom(str):pass
def arena(value):
 terms=[]
 def add(v):
  if isinstance(v,Atom):node=dict(kind='atom',value=str(v))
  elif isinstance(v,int):node=dict(kind='integer',value=str(v))
  elif isinstance(v,float):node=dict(kind='float',value=struct.pack('>d',v).hex().upper())
  elif isinstance(v,tuple):node=dict(kind='tuple',items=[add(x)for x in v])
  elif isinstance(v,list):
   if not v:node=dict(kind='nil')
   else:node=dict(kind='list',items=[add(x)for x in v],tail=add([]))
  else:raise TypeError(v)
  terms.append(node);return len(terms)-1
 ref=add(value);return dict(root=ref,terms=terms)
atom=lambda a:(Atom('atom'),0,Atom(a));integer=lambda n:(Atom('integer'),0,n)
builtin=lambda n,args=[]:(Atom('type'),0,Atom(n),args)
names='any arity atom binary bitstring bool boolean byte char dynamic float function fun identifier integer iodata iolist list map mfa module nil neg_integer non_neg_integer no_return node none nonempty_binary nonempty_bitstring nonempty_list nonempty_maybe_improper_list nonempty_string number pid port pos_integer maybe_improper_list reference string term timeout tuple fixnum unknown'.split()
forms=[builtin(n)for n in names]+[(Atom('var'),0,Atom('_')),(Atom('var'),0,Atom('A'))]
for op in ['+','-','*','div','rem','band','bor','bxor','bsl','bsr','/','==','andalso']:
 for a,b in [(1,2),(-1,3),(4,0),(1,-1),(1.5,2),(1,1.0)]:forms.append((Atom('op'),0,Atom(op),integer(a)if isinstance(a,int)else(Atom('float'),0,a),integer(b)if isinstance(b,int)else(Atom('float'),0,b)))
for op in ['+','-','bnot','not']:
 for n in [-3,0,1,1<<180]:forms.append((Atom('op'),0,Atom(op),integer(n)))
for _ in range(6000):
 a=r.choice(forms[:40]);b=r.choice(forms[:40]);choice=r.randrange(12)
 if choice==0:f=builtin('union',[a,(Atom('ann_type'),0,[(Atom('var'),0,Atom('A')),b])])
 elif choice==1:f=builtin('tuple',[a,b])
 elif choice==2:f=builtin('map',[builtin('map_field_assoc',[a,b]),builtin('map_field_exact',[b,a])])
 elif choice==3:f=builtin('fun',[builtin('product',[a,b]),a])
 elif choice==4:f=builtin('fun',[(Atom('type'),0,Atom('any')),b])
 elif choice==5:f=builtin('binary',[integer(r.randrange(-10,30)),integer(r.randrange(-5,12))])
 elif choice==6:f=builtin('range',[r.choice(forms[-94:]),r.choice(forms[-94:])])
 elif choice==7:f=builtin('record',[atom(r.choice(['rec','with space','α'])),builtin('field_type',[atom('field'),a])])
 elif choice==8:f=(Atom('remote_type'),0,[atom(r.choice(['m','α','after'])),atom(r.choice(['t','雪','case'])),[a,b]])
 elif choice==9:f=(Atom('user_type'),0,Atom(r.choice(['alias','case','α'])),[a,b])
 elif choice==10:f=(Atom('paren_type'),0,[a])
 else:f=builtin('list',[a])
 forms.append(f)
for _ in range(5000):
 bits=r.getrandbits(64);n=struct.unpack('>d',bits.to_bytes(8,'big'))[0]
 if n==n and abs(n)!=float('inf'):forms.append((Atom('op'),0,Atom('+'),(Atom('float'),0,n),integer(1)))
commands += [dict(op='form_to_string',a=dict(kind='none'),arena=arena(f))for f in forms]
p=root/'form_presentation_commands.jsonl';out=root/'form_presentation_results.jsonl';
with p.open('w')as f:
 for c in commands:f.write(json.dumps(c)+'\n')
with p.open('rb')as src,out.open('wb')as dest:subprocess.run([str(CONFIG.binary)],stdin=src,stdout=dest,check=True)
subprocess.run(['elixir',str(WORKSPACE/'scripts/type_oracle.exs'),str(p),str(out)],check=True)
