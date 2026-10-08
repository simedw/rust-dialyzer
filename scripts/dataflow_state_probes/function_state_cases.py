import json,random
r=random.Random(7331)
pool=[{'kind':k}for k in ['any','none','unit','atom','integer','number','float','binary','nil','tuple_any']]+[{'kind':'atoms','values':['ok']},{'kind':'integers','values':['42']},{'kind':'proper_list','elements':{'kind':'integer'},'nonempty':False},{'kind':'tuple','elements':[{'kind':'atoms','values':['ok']},{'kind':'integer'}]},{'kind':'nominal','name':{'symbol':'named'},'inner':{'kind':'integer'}}]
for i in range(3000):
 defs=[dict(key='top',arity=0,escaping=False,named=False)]
 for key in r.sample(range(10000),r.randrange(1,30)):
  defs.insert(r.randrange(len(defs)+1),dict(key=key,arity=r.randrange(4),escaping=r.choice([True,False]),named=r.choice([True,False]),raw=r.choice(pool) if r.randrange(2) else None))
 handled={d['key']:d['escaping'] or d['key']=='top'for d in defs};arity={d['key']:d['arity']for d in defs};ops=[]
 for j in range(r.randrange(5,45)):
  key=r.choice(defs)['key'];kind=r.choice(['pop','forward','return','capture','warning'])
  if key=='top' and kind=='capture':kind='pop'
  if kind=='return'and not handled[key]:kind='forward'
  op=dict(kind=kind,key=key)
  if kind in ['forward','return']:op['args']=[r.choice(pool)for _ in range(arity[key])]
  if kind=='return':op['result']=r.choice(pool)
  if kind=='capture':op['environment']=[[r.randrange(50),r.choice(pool)]for _ in range(r.randrange(5))]
  if kind=='forward':handled[key]=True
  ops.append(op)
 labels=[d['key']for d in defs if d['key']!='top']
 edges=[[r.choice(labels+[9999999]),r.choice(labels)]for _ in range(r.randrange(35))]
 print(json.dumps(dict(id=i,definitions=defs,operations=ops,edges=edges)))
