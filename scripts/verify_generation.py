import hashlib,json,os,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];HERE=ROOT/'.cache/generation';HERE.mkdir(parents=True,exist_ok=True)
files=list((ROOT/'src').rglob('*.rs'))+[ROOT/p for p in ['Cargo.toml','Cargo.lock','scripts/generation_probes/native_generation.exs','scripts/verify_generation.py','corpus/bifs.json','scripts/core_export.exs','scripts/type_wire.exs','scripts/type_codec.exs']]
def hashes():return {str(p):hashlib.sha256(p.read_bytes()).hexdigest()for p in files}
before=hashes();binary=ROOT/'target/release/generation_probe'
subprocess.run(['cargo','build','--release','--offline','--bin','generation_probe'],cwd=ROOT,check=True)
subprocess.run(['elixir',str(ROOT/'scripts/generation_probes/native_generation.exs'),str(ROOT),str(HERE/'generation-commands.jsonl')],check=True,env=os.environ|{'ASDF_ERLANG_VERSION':'28.3','ASDF_ELIXIR_VERSION':'1.19.4-otp-28'})
commands=(HERE/'generation-commands.jsonl').read_bytes();rows=[json.loads(x)for x in commands.splitlines()]
assert len(rows)==3560,'generator fixture count changed'
assert all(r['profile']['original_sha256']=='08945A69DD63A44CE877D696E1C27BE2753DE29AE6FC1F8E183D37AB0ADD9882' for r in rows),'different native source BEAM'
input_hash=hashlib.sha256(commands).hexdigest()
results=[json.loads(x)for x in subprocess.check_output([str(binary)],input=commands).splitlines()]
assert len(results)==len(rows),'partial generator output'
def normalize(v):
 if isinstance(v,dict):
  if 'derived'in v:return {'derived':True}
  return {k:normalize(x)for k,x in v.items()if k!='closure_checks'}
 if isinstance(v,list):return [normalize(x)for x in v]
 return v
failures=[{'index':i,'native':r['native'],'rust':s}for i,(r,s)in enumerate(zip(rows,results))if normalize(s)!=normalize(r['native'])]
closures=sum(s.get('closure_checks',0)for s in results)
expected=sum(1 for r in rows for f in r['native']['functions']for n in f['graph']['nodes']if n.get('right',{}).get('derived'))
assert expected==closures and closures>0,'incomplete source capture checks'
assert not failures,json.dumps(failures[:2])
assert before==hashes(),'source changed during generator proof'
assert hashlib.sha256((HERE/'generation-commands.jsonl').read_bytes()).hexdigest()==input_hash,'inputs changed during proof'
report={'cases':len(rows),'matched':len(rows),'closure_captures':closures,'graph_scope':'literal/var/alias/let/letrec/fun/seq/catch/tuple/cons/values plus selected native primops, scalar/generic BIF constructors, append, call and apply prototypes plus binary/segment construction, normal-mode try/catch and remaining documented primops','checked':['all normalized constraint terms and dependencies','exact original VM closure ordering keys/captures','fresh labels','exported success bindings'],'unsupported':['apply target constraints','imported signature/contract native fixtures','same SCC named call native fixtures','case/guard/clauses','guard-mode try','map'],'full_core_generation':False,'implementation_sha256':before,'inputs_sha256':{str(HERE/'generation-commands.jsonl'):input_hash}}
(HERE/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k]for k in ['cases','matched','closure_captures','full_core_generation']}))
