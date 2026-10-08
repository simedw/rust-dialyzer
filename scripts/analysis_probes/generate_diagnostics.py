import json,itertools
from pathlib import Path
import sys
P=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
P.mkdir(parents=True,exist_ok=True)
any={'kind':'any'};integer={'kind':'integer'};atom={'kind':'atom'};none={'kind':'none'};unit={'kind':'unit'};a={'kind':'atoms','values':['ok']};b={'kind':'atoms','values':['error']};union={'kind':'union','values':[integer,atom]};lst={'kind':'proper_list','elements':integer,'nonempty':False};tup={'kind':'tuple','elements':[a,integer]}
types=[any,none,unit,integer,atom,a,b,union,lst,tup]
def fun(args,ret):return {'kind':'function','domain':args,'range':ret}
def entry(m='impl',f='f',args=[integer],ret=integer,alts=None,loc=10,optional=False):
 return {'module':m,'function':f,'arity':len(args),'file':m+'.erl','location':loc,'alternatives':alts or [{'signature':fun(args,ret)}],'optional':optional}
def sig(m,f,args,ret):return {'module':m,'function':f,'arity':len(args),'arguments':args,'result':ret}
rows=[]
for ca,sa,cr,sr in itertools.product([integer,atom,any],[integer,atom,any],types,types):
 e=entry(args=[ca],ret=cr,loc=[10,3]);rows.append({'kind':'contracts','modules':[{'name':'impl','entries':[e]}],'signatures':[sig('impl','f',[sa],sr)]})
for d in [integer,atom,any]:
 for x,y,sr in itertools.product(types,types,[integer,atom,union]):
  alts=[{'signature':fun([integer],x)},{'signature':fun([d],y)}]
  rows.append({'kind':'contracts','modules':[{'name':'impl','entries':[entry(alts=alts)]}],'signatures':[sig('impl','f',[any],sr)]})
v={'kind':'var','id':{'atom':'T'}}
for sr in types:
 e=entry(alts=[{'signature':fun([v],v),'constraints':[[v,integer]]}]);rows.append({'kind':'contracts','modules':[{'name':'impl','entries':[e]}],'signatures':[sig('impl','f',[integer],sr)]})
# Source module iteration and native map iteration including HAMT transition.
for count in [0,1,2,31,32,33,64]:
 entries=[entry(f='f'+str(i),ret=atom if i%2 else integer,loc=i+2)for i in range(count)]
 rows.append({'kind':'contracts','modules':[{'name':'impl','entries':entries},{'name':'other','entries':[entry(m='other',ret=atom)]}], 'signatures':[sig('impl',e['function'],[integer],integer)for e in entries]+[sig('other','f',[integer],integer)]})
rows.append({'kind':'contracts','modules':[{'name':'impl','entries':[entry()]}],'signatures':[]})
for f,args,ret in [('abs',[integer],integer),('abs',[any],any),('is_atom',[any],atom),('raise',[any,any,any],unit)]:
 e=entry(m='erlang',f=f,args=args,ret=ret);rows.append({'kind':'contracts','modules':[{'name':'erlang','entries':[e]}],'signatures':[sig('erlang',f,args,ret)]})
def callback(carg=integer,cret=integer,arg=integer,ret=integer,decl=None,export=True,present=True,optional=False,behaviours=None):
 cb=entry(m='beh',args=[carg],ret=cret,optional=optional)
 return {'kind':'callbacks','module':'impl','filename':'impl.erl','behaviours':behaviours or [{'name':'beh','location':5}],'callbacks':{'beh':[cb]},'functions':[{'module':'impl','function':'f','arity':1,'file':'include.hrl','location':[20,4]}],'exports':[{'module':'impl','function':'f','arity':1}]if export else [],'signatures':[sig('impl','f',[arg],ret)]if present else [],'declared':[]if decl is None else [entry(args=[decl[0]],ret=decl[1],loc=[30,6])]}
for ret,arg,dr,da in itertools.product(types,[integer,atom,any],types,[integer,atom,any]):rows.append(callback(arg=arg,ret=ret,decl=(da,dr)))
for ca,cr,sa,sr in itertools.product([integer,atom,any],types,[integer,atom,any],[none,unit,integer,atom]):rows.append(callback(carg=ca,cret=cr,arg=sa,ret=sr))
for optional,present,export in itertools.product([False,True],repeat=3):rows.append(callback(present=present,export=export,optional=optional))
for repeat in [1,2,3]:
 bs=[{'name':'beh','location':7+i,'american':bool(i%2)}for i in range(repeat)]+[{'name':'missing','location':[99,2]}]
 rows.append(callback(arg=atom,ret=atom,decl=(atom,atom),behaviours=bs));rows.append(callback(present=False,behaviours=bs))
remote={'kind':'remote_form','module':'dep','name':'t'}
for sr in types:
 for form_ret in [remote,{'kind':'union','values':[integer,remote]}, {'kind':'local_form','name':'remote_alias'}]:
  e=entry(ret=union);e['alternatives'][0]['form']=fun([integer],form_ret)
  rows.append({'kind':'contracts','modules':[{'name':'impl','entries':[e],'aliases':[{'name':'remote_alias','type':{'kind':'union','values':[integer,remote]}}]}],'signatures':[sig('impl','f',[integer],sr)]})
record={'tag':'r','fields':[{'name':'value','type':integer}]};record_ty={'kind':'tuple','elements':[{'kind':'atoms','values':['r']},integer]};broad_record={'kind':'tuple','elements':[{'kind':'atoms','values':['r']},any]}
for cr,sr in itertools.product([record_ty,broad_record,union,integer,any],[record_ty,broad_record,union,integer,any]):
 e=entry(ret=cr);e['alternatives'][0]['form']=fun([integer],{'kind':'record_form','tag':'r'} if cr==record_ty else cr)
 rows.append({'kind':'contracts','modules':[{'name':'impl','entries':[e],'records':[record]}],'signatures':[sig('impl','f',[integer],sr)]})
 rows.append(dict(callback(cret=cr,ret=sr,decl=(integer,cr)),records=[record]))
opaque={'kind':'nominal','name':{'module':'dep','name':'t','arity':0,'opacity':'opaque'},'inner':integer}
for cr,sr in itertools.product([opaque,integer,any],[opaque,integer,any]):
 e=entry(ret=cr);e['alternatives'][0]['form']=fun([integer],remote if cr==opaque else cr)
 rows.append({'kind':'contracts','modules':[{'name':'impl','entries':[e]}],'signatures':[sig('impl','f',[integer],sr)]})
 c=callback(cret=cr,ret=sr);c['callbacks']['beh'][0]['alternatives'][0]['form']=fun([integer],remote if cr==opaque else cr);rows.append(c)
# Ordered callback lists, optional declarations, multiple mismatching argument positions.
for count in [1,2,5,33]:
 c=callback(arg=atom,ret=atom,decl=(atom,atom));c['callbacks']['beh']=[entry(m='beh',f='f'+str(i),args=[integer,integer],ret=integer,optional=i%3==0) for i in range(count)]
 c['functions']=[dict(module='impl',function='f'+str(i),arity=2,file='callback.hrl',location=50+i) for i in range(count)]
 c['signatures']=[sig('impl','f'+str(i),[atom,atom],atom) for i in range(count)]
 c['exports']=[dict(module='impl',function='f'+str(i),arity=2) for i in range(count)]
 c['declared']=[entry(f='f'+str(i),args=[atom,atom],ret=atom,loc=[100+i,5]) for i in range(count)]
 rows.append(c)
(P/'diagnostics-requests.jsonl').write_text(''.join(json.dumps(r)+'\n'for r in rows));print(len(rows))
