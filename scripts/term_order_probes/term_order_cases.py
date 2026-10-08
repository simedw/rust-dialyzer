import argparse,json,random,subprocess,struct,base64
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];p=ROOT/'.cache/term_order';p.mkdir(parents=True,exist_ok=True);parser=argparse.ArgumentParser();parser.add_argument('--probe',default=str(p/'target/debug/term_order_probe'));args=parser.parse_args();r=random.Random(283001)
class Atom(str):pass
class Improper:
 def __init__(self,items,tail):self.items,self.tail=items,tail
class MapPairs:
 def __init__(self,items):self.items=items
class Bits:
 def __init__(self,v,n):self.v,self.n=(int.from_bytes(v,"big")>>(8*len(v)-n)<<(8*len(v)-n)).to_bytes(len(v),"big"),n
def arena(v):
 ts=[]
 def add(v):
  if isinstance(v,Atom):t=dict(kind='atom',value=v)
  elif isinstance(v,int):t=dict(kind='integer',value=str(v))
  elif isinstance(v,float):t=dict(kind='float',value=struct.pack('>d',v).hex().upper())
  elif isinstance(v,bytes):t=dict(kind='binary',value=base64.b64encode(v).decode())
  elif isinstance(v,Bits):t=dict(kind='bitstring',bytes=base64.b64encode(v.v).decode(),bits=v.n)
  elif isinstance(v,tuple):t=dict(kind='tuple',items=[add(x)for x in v])
  elif isinstance(v,list):t=dict(kind='nil')if not v else dict(kind='list',items=[add(x)for x in v],tail=add([]))
  elif isinstance(v,Improper):t=dict(kind='list',items=[add(x)for x in v.items],tail=add(v.tail))
  elif isinstance(v,dict):t=dict(kind='map',entries=[[add(k),add(v)]for k,v in v.items()])
  elif isinstance(v,MapPairs):t=dict(kind='map',entries=[[add(k),add(v)]for k,v in v.items])
  else:raise TypeError(v)
  ts.append(t);return len(ts)-1
 root=add(v);return dict(root=root,terms=ts)
scalar=[-0.0,0.0,1,1.0,-1,-1.0,2,-2,0,2**1000,-2**1000,1e300,-1e300,1e-300,-1e-300,Atom('a'),Atom('z'),Atom('å'),Atom('𐀀'),[],b'',b'a',Bits(b'\x80',1),Bits(b'\x00',1),Bits(b'\xff',7)]
def value(d):
 if d==0 or r.randrange(3)==0:return r.choice(scalar)
 k=r.randrange(5)
 if k==0:return tuple(value(d-1)for _ in range(r.randrange(4)))
 if k==1:return [value(d-1)for _ in range(r.randrange(4))]
 if k==2:return Improper([value(d-1)for _ in range(r.randrange(4))],value(d-1))
 if k==3:return {r.choice([Atom('a'),Atom('b'),0,0.0,-0.0,1,1.0,2,-2.0]):value(d-1)for _ in range(r.randrange(4))}
 n=r.randrange(0,32);return Bits(r.getrandbits(((n+7)//8)*8).to_bytes((n+7)//8,'big'),n)
scalar += [MapPairs([(0,Atom('i')),(0.0,Atom('p')),(-0.0,Atom('n'))]),MapPairs([(1,Atom('i')),(1.0,Atom('f'))]),MapPairs([(Atom('a'),1),(Atom('a'),2)]),MapPairs([(Atom('a'),1.0)]),MapPairs([(Atom('a'),1)])]
pool=scalar+[value(3)for _ in range(500)]
commands=[dict(a=arena(a),b=arena(b),exact=exact)for a in scalar for b in scalar for exact in [False,True]]
commands +=[dict(a=arena(r.choice(pool)),b=arena(r.choice(pool)),exact=bool(r.randrange(2)))for _ in range(30000)]
def deep(kind,n,leaf,shared=False):
 terms=[dict(kind='atom',value=leaf),dict(kind='nil')];root=0
 for _ in range(n):
  if kind=='tuple':term=dict(kind='tuple',items=[root,root]if shared else[root])
  elif kind=='list':term=dict(kind='list',items=[root],tail=1)
  else:term=dict(kind='map',entries=[[0,root]])
  root=len(terms);terms.append(term)
 return dict(root=root,terms=terms)
for kind,n,shared in [('tuple',50000,False),('list',50000,False),('map',50000,False),('tuple',10000,True)]:
 for exact in [False,True]:
  commands.append(dict(a=deep(kind,n,'a',shared),b=deep(kind,n,'b',shared),exact=exact))
  if not shared:commands.append(dict(a=deep(kind,n,'a'),b=deep(kind,n,'a'),exact=exact))
with(p/'term_order_commands.jsonl').open('w')as f:
 for c in commands:f.write(json.dumps(c)+'\n')
with(p/'term_order_commands.jsonl').open('rb')as inp,(p/'term_order_results.jsonl').open('wb')as out:subprocess.run([args.probe],stdin=inp,stdout=out,check=True)
subprocess.run(['elixir',str(ROOT/'scripts/term_order_probes/term_order_oracle.exs'),str(p/'term_order_commands.jsonl'),str(p/'term_order_results.jsonl')],check=True)
