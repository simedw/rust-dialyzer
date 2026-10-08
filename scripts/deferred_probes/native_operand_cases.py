import json,random
from cache_fixture_lib import A,arena,itype
c=lambda tag,elements=A('any'),qual=A('unknown'):(A('c'),A(tag),elements,qual)
values=[A('any'),A('none'),A('unit'),A('opaque'),A('bad_cached'),A('other'),0,1,(),[],[A('x')],(A('bad'),A('record')),c('var',A('X')),c('var',(A('arbitrary'),1),A('bad_qual')),c('unexpected',1),c('unexpected',2),c('atom',[A('a')]),c('atom',[A('a')],A('wrong')),c('atom',A('bad')),itype(1),c('number',A('weird'),A('bad')),c('nil',[],A('unknown')),c('nil',A('arbitrary'),A('wrong')),c('list',(A('bad'),A('bad')),A('nonempty')),c('list',(A('bad'),A('bad'))),c('product',[A('bad')]),c('product',A('bad')),c('binary',(0,0)),c('binary',(-1,-8)),c('binary',A('bad')),c('tuple',A('any'),(A('any'),A('any'))),c('tuple',[A('bad')],(1,A('bad'))),c('tuple',A('invalid'),(1,A('bad'))),c('tuple',A('invalid'),(2,A('bad'))),c('tuple_set',A('bad')),c('union',A('bad')),c('nominal',(A('name'),A('bad'))),c('nominal_set',(A('bad'),A('bad'))),c('map',(A('bad'),A('bad'),A('bad'))),c('identifier',A('bad')),c('function',(A('bad'),A('bad')))]
# Add ordinary canonical families and raw record variants of every native family.
for i in range(50):values.append(itype(i-25))
rows=[dict(op='impossible',left=arena(x))for x in values]
rows.extend(dict(op='inf',left=arena(a),right=arena(b))for a in values for b in values)
r=random.Random(11317)
for i in range(5000):
 tag=r.choice(['atom','binary','function','identifier','list','map','nil','nominal','nominal_set','number','product','tuple','tuple_set','union','var','unknown'])
 x=c(tag,r.choice([A('bad'),(r.randrange(-10,10),r.randrange(-10,10)),[],[A('x')],r.randrange(-10,10)]),r.choice([A('unknown'),A('weird'),A('nonempty'),(r.randrange(3),A('weird'))]))
 y=r.choice(values);rows.append(dict(op='inf',left=arena(x),right=arena(y)));rows.append(dict(op='inf',left=arena(y),right=arena(x)))
for size in [200,20000]:
 ts=[dict(kind='atom',value='deep_leaf')]
 for _ in range(size):ts.append(dict(kind='tuple',items=[len(ts)-1]))
 deep=dict(root=len(ts)-1,terms=ts)
 rows += [dict(op='inf',left=arena(A('any')),right=deep),dict(op='inf',left=deep,right=deep),dict(op='impossible',left=deep)]
ts=[dict(kind='atom',value='shared_leaf')]
for _ in range(100):ts.append(dict(kind='tuple',items=[len(ts)-1,len(ts)-1]))
dag=dict(root=len(ts)-1,terms=ts)
rows += [dict(op='inf',left=arena(A('any')),right=dag,wire_identity='right'),dict(op='impossible',left=dag)]
open('native_operand_dag_equality_repro.jsonl','w').write(json.dumps(dict(op='inf',left=dag,right=dag))+'\n')
open('native_operand_commands.jsonl','w').writelines(json.dumps(c)+'\n'for c in rows)
