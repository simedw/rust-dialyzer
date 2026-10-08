import json,random,itertools,subprocess
from pathlib import Path
WORKSPACE=Path(__file__).resolve().parents[2];root=WORKSPACE/".cache/types";r=random.Random(28312)
t=lambda term,**kw:dict(term=term,**kw)
base=[t('nil')]+[t('atom',value=a)for a in ['a','b','false','true','nil']]+[t('integer',value=str(n))for n in [-2,-1,0,1,2,1<<180]]+[t('float',value=n)for n in [-1.0,-0.0,0.0,0.5,1.0,2.5]]+[t('bitstring',bits=n,value=str(v))for n in [0,1,7,8,9,24]for v in [0,(1<<n)-1]]+[t('function',arity=n,identity='f'+str(n))for n in range(4)]+[t('pid',identity='self'),t('port',identity='standard'),t('reference',identity='a'),t('reference',identity='b')]
def gen(d):
 if d<=0 or r.random()<.4:return r.choice(base)
 choice=r.randrange(3)
 if choice==0:return t('tuple',elements=[gen(d-1)for _ in range(r.randrange(5))])
 if choice==1:return t('cons',head=gen(d-1),tail=gen(d-1))
 return t('map',pairs=[[gen(d-1),gen(d-1)]for _ in range(r.randrange(6))])
terms=base+[t('map',pairs=[[a,b],[b,a],[a,a]])for a,b in itertools.product(base,base)]+[gen(3)for _ in range(12000)]
commands=[dict(op='from_runtime_term',a=dict(kind='none'),term=term)for term in terms]
p=root/'runtime_commands.jsonl';out=root/'runtime_results.jsonl';p.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with p.open('rb')as src,out.open('wb')as dest:subprocess.run([str(WORKSPACE/'target/release/type_probe')],stdin=src,stdout=dest,check=True)
subprocess.run(['elixir',str(WORKSPACE/'scripts/type_oracle.exs'),str(p),str(out)],check=True)
