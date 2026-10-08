import argparse,json,random,subprocess,struct
from pathlib import Path
p=Path(__file__).resolve().parent;parser=argparse.ArgumentParser();parser.add_argument('--probe',default=str(p/'target/debug/cache_probe'));args=parser.parse_args();r=random.Random(28322)
from cache_fixture_lib import Atom as A,arena,integer,var,builtin,user,remote,table
cases=[]
for i in range(500):
 old=r.randrange(-100,100);new=old+r.randrange(1,20)
 snapshots=[{A('m'):table('m',old),A('n'):table('n',old)},{A('m'):table('m',new),A('n'):table('n',old)}]
 exports=arena([(A(m),A(n),a)for m in ['m','n']for n,a in [('a',0),('outer',1),('opaque',0),('inner',1),('loop',0)]])
 step=lambda snapshot,form,mode='resolve',**kw:dict(snapshot=snapshot,form=arena(form),mode=mode,**kw)
 steps=[step(0,builtin('map')),step(0,(A('type'),0,A('map'),A('any'))),step(1,user('a')),step(1,builtin('record',[(A('atom'),0,A('r'))])),step(1,user('opaque')),step(1,user('outer',[integer(old)])),step(1,user('outer',[remote('n','a')])),step(1,user('outer',[integer(new)])),step(1,remote('n','a'),mode='without_remote'),step(1,user('a'),mode='check_remote'),step(1,user('a')),step(1,builtin('record',[(A('atom'),0,A('r'))]),mode='check_fields')]
 steps += [step(1,user('a'),site=dict(kind='type',module='m',name='a',arity=0)),step(1,user('outer',[integer(new)]),site=dict(kind='type',module='m',name='outer',arity=1)),step(1,user('a'),site=dict(kind='record',module='m',name='r',arity=1)),step(1,user('a'),site=dict(kind='type',module='n',name='a',arity=0)),step(1,user('inner',[integer(new)])),step(1,user('inner',[integer(old)]))]
 name=(1<<100)+i
 steps += [step(1,user('loop')),step(1,user('loop'))]
 steps+=[step(1,var(name)),step(1,var(name),variables=[dict(name=dict(kind='integer',value=str(name)),type=dict(kind='integers',values=[str(new)]))])]
 cases.append(dict(snapshots=[arena(s)for s in snapshots],exports=exports,steps=steps))
with(p/'cache_commands.jsonl').open('w')as f:
 for c in cases:f.write(json.dumps(c)+'\n')
with(p/'cache_commands.jsonl').open('rb')as inp,(p/'cache_results.jsonl').open('wb')as out:subprocess.run([args.probe],stdin=inp,stdout=out,check=True)
subprocess.run(['elixir',str(p/'cache_oracle.exs'),str(p/'cache_commands.jsonl'),str(p/'cache_results.jsonl')],check=True)
