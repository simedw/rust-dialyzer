import json,random,itertools,subprocess
from pathlib import Path
from probe_config import configuration
WORKSPACE=Path(__file__).resolve().parents[2];CONFIG=configuration(WORKSPACE);root=CONFIG.out;r=random.Random(28310)
k=lambda kind,**kw:dict(kind=kind,**kw)
none=k('none');unit=k('unit');anyt=k('any');opaque=k('opaque');atom=k('atoms',values=['a']);integer=k('integers',values=['1']);nil=k('nil');var=k('var',id=dict(atom='x'))
keys=[none,unit,anyt,opaque,atom,integer,nil,var,k('atom'),k('number'),k('integer'),k('float'),k('atoms',values=['a','b']),k('integers',values=['1','2']),k('range',**{'from':'0','to':'255'}),k('tuple',elements=[atom]),k('map_any'),k('proper_list',elements=anyt,nonempty=False),k('nominal',name=dict(symbol='a'),inner=atom)]
defaults=[none,unit,anyt,opaque,atom,integer,nil,var,k('atom'),k('integer'),k('range',**{'from':'0','to':'255'}),k('nominal',name=dict(symbol='a'),inner=k('proper_list',elements=anyt,nonempty=False))]
values=[none,unit,anyt,atom,integer,opaque]
commands=[]
def m(es,dk,dv):return k('map',entries=es,default_key=dk,default_value=dv)
for key,dk,dv,value,p in itertools.product(keys,defaults,values,values,['mandatory','optional']):commands.append(dict(op='canonical',a=m([dict(key=key,presence=p,value=value)],dk,dv)))
for _ in range(6000):
 es=[dict(key=r.choice(keys),presence=r.choice(['mandatory','optional']),value=r.choice(values))for _ in range(r.randrange(5))]
 commands.append(dict(op='canonical',a=m(es,r.choice(defaults),r.choice(values))))
for key,p,value,dk,dv in itertools.product([atom,integer,nil],['mandatory','optional'],values,defaults,values):commands.append(dict(op='widen_to_number',a=m([dict(key=key,presence=p,value=value)],dk,dv)))
p=root/'map_candidate_commands.jsonl';out=root/'map_candidate_results.jsonl';p.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with p.open('rb')as src,out.open('wb')as dest:subprocess.run([str(CONFIG.binary)],stdin=src,stdout=dest,check=True)
subprocess.run(['elixir',str(WORKSPACE/'scripts/type_oracle.exs'),str(p),str(out)],check=True)
