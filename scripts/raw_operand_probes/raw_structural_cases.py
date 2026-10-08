import json,random
from raw_scalar_cases import A,arena,Improper
c=lambda tag,e,q=A('unknown'):(A('c'),A(tag),e,q)
scalar=[A('any'),A('none'),A('unit'),A('bad'),c('atom',[A('a')]),c('atom',[A('b')]),c('atom',A('bad'))]
products=[c('product',x,q)for q in [A('unknown'),A('weird')]for x in [A('bad'),[],[A('any')],[A('none')],[A('unit')],[A('bad')],[A('any'),A('bad')],Improper([A('any')],A('tail'))]]
functions=[c('function',(d,r),q)for q in [A('unknown'),A('weird')]for d in scalar+products[:8]for r in scalar]
rows=[dict(op='inf',left=arena(a),right=arena(b))for values in [products,functions]for a in values for b in values]
r=random.Random(28602)
for _ in range(2000):
 def make(depth):
  if depth==0:return r.choice(scalar)
  if r.randrange(2):return c('function',(make(depth-1),make(depth-1)),A('weird'))
  return c('product',[make(depth-1)for _ in range(r.randrange(3))],A('weird'))
 rows.append(dict(op='inf',left=arena(make(3)),right=arena(make(3))))
open('raw_structural_commands.jsonl','w').writelines(json.dumps(x)+'\n'for x in rows)
print(len(rows))
