import json,random,struct
from cache_fixture_lib import A
class Improper:
 def __init__(self,items,tail):self.items,self.tail=items,tail
def arena(value):
 ts=[]
 def add(v):
  if isinstance(v,A):t=dict(kind='atom',value=str(v))
  elif isinstance(v,int):t=dict(kind='integer',value=str(v))
  elif isinstance(v,float):t=dict(kind='float',value=struct.pack('>d',v).hex().upper())
  elif isinstance(v,tuple):t=dict(kind='tuple',items=[add(x)for x in v])
  elif isinstance(v,Improper):t=dict(kind='list',items=[add(x)for x in v.items],tail=add(v.tail))
  elif isinstance(v,list):t=dict(kind='nil')if not v else dict(kind='list',items=[add(x)for x in v],tail=add([]))
  else:raise TypeError(v)
  ts.append(t);return len(ts)-1
 root=add(value);return dict(root=root,terms=ts)
c=lambda tag,e,q=A('unknown'):(A('c'),A(tag),e,q)
if __name__ == "__main__":
 sets=[A('any'),A('none'),A('bad'),[],[1],[1.0],[2,1],[1,1],[A('x'),A('a')],[1,1.0,2],[2.0,1.0],Improper([1],A('tail')),Improper([1,2],A('tail')),(A('bad'),),[1<<150,A('z')]]
 rows=[]
 for tag in ['atom','identifier']:
  for s in sets:
   for t in sets:
    for qualifier in [A('unknown'),A('weird')]:rows.append(dict(op='inf',left=arena(c(tag,s,qualifier)),right=arena(c(tag,t))))
 num=[]
 for q in [A('unknown'),A('integer'),A('float'),A('bad')]:
  for e in [A('any'),A('bad'),[],0,(A('int_set'),[]),(A('int_set'),[1]),(A('int_set'),A('bad'))]:num.append(c('number',e,q))
 for s in sets:num.append(c('number',(A('int_set'),s),A('integer')))
 for a in [A('neg_inf'),A('pos_inf'),A('bad'),-5,0,5,1.0]:
  for b in [A('neg_inf'),A('pos_inf'),A('bad'),-1,4,50,2.0]:num.append(c('number',(A('int_rng'),a,b),A('integer')))
 for x in num:
  for y in num:rows.append(dict(op='inf',left=arena(x),right=arena(y)))
 r=random.Random(28601)
 for _ in range(5000):
  x=[r.choice([1,1.0,2,2.0,-1,A('a'),A('b'),(1,)])for _ in range(r.randrange(8))]
  y=[r.choice([1,1.0,2,2.0,-1,A('a'),A('b'),(1,)])for _ in range(r.randrange(8))]
  tag=r.choice(['atom','identifier']);rows.append(dict(op='inf',left=arena(c(tag,x,A('bad'))),right=arena(c(tag,y,A('bad')))))
 open('raw_scalar_commands.jsonl','w').writelines(json.dumps(row)+'\n'for row in rows)
 print(len(rows))
