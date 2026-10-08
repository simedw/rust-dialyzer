class Atom(str):pass
A=Atom
def arena(v):
 ts=[]
 def add(v):
  if isinstance(v,A):t=dict(kind='atom',value=str(v))
  elif isinstance(v,int):t=dict(kind='integer',value=str(v))
  elif isinstance(v,tuple):t=dict(kind='tuple',items=[add(x)for x in v])
  elif isinstance(v,list):t=dict(kind='nil')if not v else dict(kind='list',items=[add(x)for x in v],tail=add([]))
  elif isinstance(v,dict):t=dict(kind='map',entries=[[add(k),add(v)]for k,v in v.items()])
  else:raise TypeError(v)
  ts.append(t);return len(ts)-1
 root=add(v);return dict(root=root,terms=ts)
integer=lambda n:(A('integer'),0,n)
var=lambda n:(A('var'),0,n)
builtin=lambda n,a=[]:(A('type'),0,A(n),a)
user=lambda n,a=[]:(A('user_type'),0,A(n),a)
remote=lambda m,n,a=[]:(A('remote_type'),0,[(A('atom'),0,A(m)),(A('atom'),0,A(n)),a])
itype=lambda n:(A('c'),A('number'),(A('int_set'),[n]),A('integer'))
def table(mod,k):
 alias=lambda name,arity,body,params,kind='type':((A(kind),A(name),arity),((A(mod),([ord(c)for c in 'cache.erl'],1),body,params),A('any')))
 rows=dict([alias('loop',0,builtin('union',[user('loop')]+[integer(n)for n in range(20)]),[]),alias('a',0,integer(k),[]),alias('outer',1,var(A('T')),[A('T')]),alias('inner',1,var(1<<100),[1<<100]),alias('opaque',0,integer(k),[],kind='opaque')])
 rows[(A('record'),A('r'))]=(([ord(c)for c in 'cache.erl'],1),[(1,[(A('f'),user('a'),itype(k))])])
 return rows
