import json, subprocess, os, random
from pathlib import Path
root=Path(__file__).resolve().parents[2];h=root/'.cache/constraints';h.mkdir(parents=True,exist_ok=True);rng=random.Random(710)
key=lambda n:{'integer':str(n)};var=lambda n:{'kind':'var','id':key(n)};atom=lambda n:{'kind':'atoms','values':[n]}
binary=root/'target/release/constraints_probe'
assert binary.stat().st_mtime >= max(p.stat().st_mtime for p in (root/'src').rglob('*.rs')), 'Rust probe stale; rebuild before verification'
commands=[]
pool=[{'kind':k}for k in ['any','none','unit','atom','integer','float','nil']]+[atom('a'),atom('b'),{'kind':'integers','values':['0']}]
for _ in range(2000):
 nodes=[];arity=rng.randrange(4)
 def atomic(left,right,relation='eq'):
  index=len(nodes);nodes.append({'kind':'atomic','left':left,'right':right,'relation':relation});return index
 children=[]
 for n in range(1,arity+2):
  if rng.random()<.4:
   lhs=atomic(var(n),rng.choice(pool),'sub');rhs=atomic(var(n),rng.choice(pool),'sub')
   branch=len(nodes);nodes.append({'kind':'disj','children':[lhs,rhs]});children.append(branch)
  else:children.append(atomic(var(n),rng.choice(pool),'sub'))
 children.append(atomic(var(100),{'kind':'function','domain':[var(n)for n in range(1,arity+1)],'range':var(arity+1)}))
 nodes.append({'kind':'conj','children':children});functions=[{'function':key(100),'body':len(nodes)-1,'arity':arity}]
 commands.append({'normalize':{'nodes':nodes,'root':len(nodes)-1,'functions':functions,'solve':True}})
inp=h/'prepared-solve.jsonl';inp.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with inp.open('rb')as f:result=subprocess.check_output([str(binary)],stdin=f)
rust=[json.loads(x)for x in result.splitlines()];(h/'prepared-solve.rust.jsonl').write_bytes(result)
env=os.environ.copy();env.update(ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28');subprocess.run(['elixir',str(root/'scripts/constraint_probes/native_normalize.exs'),str(root),str(inp),str(h/'prepared-solve.native.json')],env=env,check=True)
native=json.loads((h/'prepared-solve.native.json').read_text());assert len(rust)==len(native)==len(commands)==2000
failures=[{'index':i,'command':commands[i],'rust':r,'native':n}for i,(r,n)in enumerate(zip(rust,native))if{k:v for k,v in r.items()if k!='detail'}!={k:v for k,v in n.items()if k!='detail'}]
(h/'prepared-solve-verification.json').write_text(json.dumps({'commands':len(commands),'passed':len(commands)-len(failures),'failures':failures},indent=2));print(len(commands),len(failures))
