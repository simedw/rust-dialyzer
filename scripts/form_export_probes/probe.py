import argparse,json,random,subprocess,struct
from pathlib import Path
scripts=Path(__file__).resolve().parent;root=scripts.parents[1];p=root/'.cache/form_exports';p.mkdir(parents=True,exist_ok=True);parser=argparse.ArgumentParser();parser.add_argument('--probe',default=str(p/'target/debug/metadata_exports_probe'));args=parser.parse_args();r=random.Random(28321)
class Atom(str):pass
A=Atom
commands=[]
for l in Path('/private/tmp/rust-dialyzer-lattice/form_presentation_commands.jsonl').open():
 c=json.loads(l);c['op']='deps';commands.append(c)
def make(value,extra=[]):
 ts=[]
 def add(v):
  if isinstance(v,Atom):t={'kind':'atom','value':str(v)}
  elif isinstance(v,int):t={'kind':'integer','value':str(v)}
  elif isinstance(v,float):t={'kind':'float','value':struct.pack('>d',v).hex().upper()}
  elif isinstance(v,tuple):t={'kind':'tuple','items':[add(x)for x in v]}
  elif isinstance(v,dict):t={'kind':'map','entries':[[add(k),add(v)]for k,v in v.items()]}
  elif isinstance(v,list):t={'kind':'nil'}if not v else{'kind':'list','items':[add(x)for x in v],'tail':add([])}
  else:raise ValueError(v)
  ts.append(t);return len(ts)-1
 root=add(value);extras=[add(x)for x in extra];return {'root':root,'terms':ts},extras
remote=lambda m:(A('remote_type'),0,[(A('atom'),0,A(m)),(A('atom'),0,A('t')),[]])
for i in range(5000):
 sizes=sorted(r.sample(range(1,9),r.randrange(1,5)))
 fields=lambda n:[(A('f'+str(j)),remote(r.choice(['a','b','c'])),A('any'))for j in range(n)]
 table={(A('record'),A('rec')):(0,[(n,fields(n))for n in sizes])}
 arena,ids=make(table,[r.randrange(1,10)])
 commands.append({'op':'lookup','arena':arena,'tag':'rec','arity':ids[0]})
 names=[r.choice([A('A'),A('B'),A('_'),i,1<<90])for _ in range(r.randrange(0,5))]
 arena,_=make([(A('var'),0,n)for n in names]);commands.append({'op':'vars','arena':arena})
 table.update({(A('type'),A('t'),0):((A('m'),0,remote('a'),[]),A('any'))})
 arena,ids=make(table,[A(r.choice(['type','opaque','record'])),A(r.choice(['t','missing'])),r.randrange(2)])
 commands.append({'op':'defined','arena':arena,'kind':ids[0],'name':ids[1],'arity':ids[2]})
 # Table insertion order differs native map ordering; send native ordered table to preserve bridge contract.
 table=dict(sorted(table.items()))
 arena,_=make(table);commands.append({'op':'table_deps','arena':arena})

for arity in [0.0,-0.0,1.0,2.0,2.5,8.0,1e100]:
 table={(A('record'),A('rec')):(0,[(n,[])for n in [1,2,8]])}
 arena,ids=make(table,[arity]);commands.append({'op':'lookup','arena':arena,'tag':'rec','arity':ids[0]})
for count in [9999,10000,10001]:
 pair=(A('type'),0,A('map_field_assoc'),[remote('key'),remote('value')])
 arena,_=make((A('type'),0,A('map'),[pair]*count));commands.append({'op':'deps','arena':arena})
for name in ['list','nonempty_list','tuple']:
 for depth in [15,16,17,50]:
  form=remote('deep')
  for _ in range(depth):form=(A('type'),0,A(name),[form])
  arena,_=make(form);commands.append({'op':'deps','arena':arena})

for depth in [50000]:
 arena,_=make(remote('deep_parenthesis'))
 ts=arena['terms'];root=arena['root'];tag=len(ts);ts.append(dict(kind='atom',value='paren_type'));anno=len(ts);ts.append(dict(kind='integer',value='0'));nil=len(ts);ts.append(dict(kind='nil'))
 for _ in range(depth):
  list_id=len(ts);ts.append(dict(kind='list',items=[root],tail=nil));root=len(ts);ts.append(dict(kind='tuple',items=[tag,anno,list_id]))
 arena['root']=root;commands.append(dict(op='deps',arena=arena))
commands += [dict(op='cache_new'),dict(op='var_table_new')]
with (p/'commands.jsonl').open('w')as f:
 for c in commands:f.write(json.dumps(c)+'\n')
with (p/'commands.jsonl').open('rb')as inp,(p/'rust.jsonl').open('wb')as out:subprocess.run([args.probe],stdin=inp,stdout=out,check=True)

subprocess.run(['elixir',str(scripts/'oracle.exs'),str(p/'commands.jsonl'),str(p/'rust.jsonl')],check=True)
