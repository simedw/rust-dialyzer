import json,random,itertools,subprocess
from pathlib import Path
from probe_config import configuration, read_commands
WORKSPACE=Path(__file__).resolve().parents[2];CONFIG=configuration(WORKSPACE);root=CONFIG.out;r=random.Random(28314)
values={}
for file in ['commands.jsonl','family_commands.jsonl','callable_commands.jsonl','map_commands.jsonl','nominal_commands.jsonl','raw_commands.jsonl','subst_commands.jsonl']:
 for line in read_commands(root/file, CONFIG):
  a=json.loads(line)['a'];values[json.dumps(a,sort_keys=True)]=a
pool=list(values.values());k=lambda kind,**kw:dict(kind=kind,**kw);atom=lambda a:k('atoms',values=[a]);none=k('none');anyt=k('any');unit=k('unit')
atoms=['','a','A','with space','a@B_1','after','maybe','else','true','false','undefined','α','雪','é','É','ß','÷','ÿ','a\\b',"a'b",'💡']+[chr(n)for n in range(160)]+[chr(n)for n in range(160,256)]+['a'+chr(n)for n in [0,127,160,255,256,65535,128161]]
commands=[dict(op='type_to_string',a=a)for a in pool]
commands += [dict(op='atom_to_string',a=none,name=a)for a in atoms]
commands += [dict(op='type_to_string',a=atom(a))for a in atoms]
commands += [dict(op='type_to_string',a=k('nominal',name=dict(module=a,name=b,arity=2,opacity=p),inner=atom('ok')))for a,b,p in itertools.product(atoms[::8],atoms[::9],['opaque','transparent'])]
fields=[anyt,none,unit,atom('undefined'),atom('a'),k('integer'),k('range',**{'from':'0','to':'1114111'}),k('nominal',name=dict(module='m',name='t',arity=0,opacity='opaque'),inner=k('integer'))]
records=[dict(name='rec',fields=[dict(name='a',type=k('integer')),dict(name='quoted field',type=atom('undefined'))]),dict(name='rec',fields=[dict(name='only',type=k('atom'))]),dict(name='α',fields=[dict(name='λ',type=k('integer'))])]
for elements in itertools.product(fields,repeat=2):
 a=k('tuple',elements=[atom('rec'),*elements]);commands += [dict(op=op,a=a,records=records)for op in ['type_to_string','record_field_diffs']]
for a in r.sample(pool,1500):commands += [dict(op=op,a=a,records=records)for op in ['type_to_string','record_field_diffs']]
for _ in range(4000):
 n=r.randrange(5);tag=r.choice(atoms[:20]);rs=[dict(name=tag,fields=[dict(name=r.choice(atoms),type=r.choice(pool))for _ in range(n)])]
 a=k('tuple',elements=[atom(tag)]+[r.choice(fields)for _ in range(n)])
 commands += [dict(op=op,a=a,records=rs)for op in ['type_to_string','record_field_diffs']]
p=root/'presentation_commands.jsonl';out=root/'presentation_results.jsonl';p.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with p.open('rb')as src,out.open('wb')as dest:subprocess.run([str(CONFIG.binary)],stdin=src,stdout=dest,check=True)
subprocess.run(['elixir',str(WORKSPACE/'scripts/type_oracle.exs'),str(p),str(out)],check=True)
