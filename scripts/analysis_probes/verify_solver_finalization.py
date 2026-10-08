import hashlib, json, os, random, subprocess
from pathlib import Path
here=Path(__file__).resolve().parent
root=here.parents[1]
h=root/'.cache/solver_finalization';h.mkdir(parents=True,exist_ok=True)
rng=random.Random(710)
key=lambda n:{'integer':str(n)}
var=lambda n:{'kind':'var','id':key(n)}
atom=lambda n:{'kind':'atoms','values':[n]}
binary=root/'target/release/constraints_probe'
files=[Path(x) for x in (root/'target/release/constraints_probe.d').read_text().splitlines()[0].split(': ',1)[1].split()]+[here/'native_solver_finalization.exs',here/'verify_solver_finalization.py']+[root/'scripts'/x for x in ['type_wire.exs','type_codec.exs','core_export.exs']]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):sha(p) for p in files}
env=os.environ|{'ASDF_ERLANG_VERSION':'28.3','ASDF_ELIXIR_VERSION':'1.19.4-otp-28'}
beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_typesig,:erl_types],fn m->IO.puts(:code.which(m))end)'],env=env,text=True).splitlines();files.extend(map(Path,beams))
before=hashes();binary_sha=sha(binary)
assert binary.stat().st_mtime_ns >= max(p.stat().st_mtime_ns for p in files if p.suffix=='.rs'),'stale probe'
commands=[]
for _ in range(1000):
 nodes=[];functions=[]
 for f in range(rng.randrange(1,7)):
  available=[]
  for a in range(rng.randrange(1,8)):
   available.append(len(nodes));nodes.append({'kind':'atomic','left':var(1+rng.randrange(5)),'right':atom('a'+str(rng.randrange(6))),'relation':rng.choice(['eq','sub'])})
  if f and rng.random()<.8:
   for target in rng.sample(list(range(f)),rng.randrange(1,f+1)):
    available.append(len(nodes));nodes.append({'kind':'ref','function':key(100+target),'deps':[key(n)for n in [1,2,3,4,5,100+target]]})
  for _ in range(rng.randrange(0,4)):
   pick=rng.sample(available,rng.randrange(1,len(available)+1));available.append(len(nodes));nodes.append({'kind':rng.choice(['conj','disj']),'children':pick})
  nodes.append({'kind':'conj','children':available});functions.append({'function':key(100+f),'body':len(nodes)-1})
 commands.append({'normalize':{'nodes':nodes,'root':len(nodes)-1,'functions':functions}})
nodes=[{'kind':'atomic','left':var(1),'right':atom('first'),'relation':'sub'}];prev=0
for n in range(500):
 leaf=len(nodes);nodes.append({'kind':'atomic','left':var(1),'right':atom('a'+str(n)),'relation':'sub'})
 nodes.append({'kind':'disj','children':[prev,leaf]});branch=len(nodes)-1
 nodes.append({'kind':'conj','children':[branch,leaf]});prev=len(nodes)-1
commands.append({'normalize':{'nodes':nodes,'root':prev,'functions':[{'function':key(100),'body':prev}]}})

selections=[[],['v2'],['v2','v2'],['v1'],['v1','v2'],['v2','v1'],['v1','v1']]
commands=[{'normalize':dict(c['normalize'],solvers=s)} for c in commands for s in selections]
inp=h/'solver-finalization-commands.jsonl'
inp.write_text(''.join(json.dumps(c)+'\n' for c in commands))
input_sha=sha(inp)
env=os.environ|{'ASDF_ERLANG_VERSION':'28.3','ASDF_ELIXIR_VERSION':'1.19.4-otp-28'}
original_sha=subprocess.check_output(['elixir','-e',':code.which(:dialyzer_typesig) |> File.read!() |> then(&:crypto.hash(:sha256,&1)) |> Base.encode16() |> IO.puts()'],env=env,text=True).strip()
assert original_sha=='08945A69DD63A44CE877D696E1C27BE2753DE29AE6FC1F8E183D37AB0ADD9882'
subprocess.run(['elixir',str(here/'native_solver_finalization.exs'),str(root),str(inp),str(h/'solver-finalization-native.json')],env=env,check=True)
result=subprocess.check_output([str(binary)],input=inp.read_bytes())
(h/'solver-finalization-rust.jsonl').write_bytes(result)
rust=[json.loads(x) for x in result.splitlines()]
native=json.loads((h/'solver-finalization-native.json').read_text())
assert len(rust)==len(native)==len(commands)==7007
strip=lambda v:{k:x for k,x in v.items() if k!='detail'}
failures=[{'index':i,'command':commands[i],'rust':r,'native':n} for i,(r,n) in enumerate(zip(rust,native)) if strip(r)!=strip(n)]
(h/'solver-finalization-failures.json').write_text(json.dumps(failures[:20],indent=2))
assert not failures,len(failures)
assert all(n['status']=='prepared' for n in native)
differences=sum(native[i]!=native[i+3] for i in range(0,len(native),7))
assert differences==1001
assert before==hashes(),'sources changed during proof'
assert binary_sha==sha(binary) and input_sha==sha(inp)
report={'cases':7007,'matched':7007,'policy_difference_cases':differences,'original_beam_sha256':original_sha,'source_sha256':before,'binary_sha256':binary_sha,'input_sha256':input_sha,'checked':['original enumerate_constraints and order_fun_constraints','complete finalized graphs, masks, dependencies and list identifiers','default, explicit, mixed and repeated v1/v2 selections','1000 deterministic lexical function DAGs and one 500-level nested graph'],'full_core_generation':False}
(h/'solver-finalization-verification.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:report[k] for k in ['cases','matched','policy_difference_cases']}))
