import hashlib,json,os,subprocess
from pathlib import Path
H=Path(__file__).resolve().parent;R=H.parents[1];P=R/'.cache/generation_record3';P.mkdir(parents=True,exist_ok=True);binary=R/'target/release/generation_probe';direct=R/'target/release/record_argument_probe'
files=list(set([Path(x)for target in [binary,direct]for x in target.with_suffix('.d').read_text().splitlines()[0].split(': ',1)[1].split()]))+[H/'native_generation_record3.exs',H/'native_record_argument.exs',H/'source_capture_wire.exs',H/'verify_generation_record3.py']+[R/'scripts'/x for x in ['core_export.exs','type_wire.exs','type_codec.exs']]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):sha(p)for p in files}
env=os.environ|{'ASDF_ERLANG_VERSION':'28.3','ASDF_ELIXIR_VERSION':'1.19.4-otp-28'}
beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_typesig,:erl_types,:erl_bif_types,:cerl,:cerl_trees,:dialyzer_codeserver,:dialyzer_plt],fn m->IO.puts(:code.which(m))end)'],env=env,text=True).splitlines();files.extend(map(Path,beams))
before=hashes();binaries={str(p):sha(p)for p in [binary,direct]};
for target in [binary,direct]:
 own=[Path(x)for x in target.with_suffix('.d').read_text().splitlines()[0].split(': ',1)[1].split()]
 assert target.stat().st_mtime_ns>=max(p.stat().st_mtime_ns for p in own if p.suffix=='.rs'),'stale probe'
env=os.environ|{'ASDF_ERLANG_VERSION':'28.3','ASDF_ELIXIR_VERSION':'1.19.4-otp-28'}
subprocess.run(['elixir',str(H/'native_generation_record3.exs'),str(R),str(P/'generation-record3-commands.jsonl')],check=True,env=env)
subprocess.run(['elixir',str(H/'native_record_argument.exs'),str(R),str(P/'record-argument-commands.jsonl')],check=True,env=env)
data=(P/'generation-record3-commands.jsonl').read_bytes();rows=[json.loads(x)for x in data.splitlines()];assert len(rows)==384
actual=[json.loads(x)for x in subprocess.check_output([str(binary)],input=data).splitlines()];assert len(actual)==len(rows)
def norm(v):
 if isinstance(v,dict):
  if 'derived'in v:return {'derived':True}
  return {k:norm(x)for k,x in v.items()if k not in ['closure_checks','source_state_matched']}
 if isinstance(v,list):return [norm(x)for x in v]
 return v
failures=[{'index':i,'native':n['native'],'rust':a}for i,(n,a)in enumerate(zip(rows,actual))if norm(n['native'])!=norm(a)or a.get('source_state_matched')is not True]
(P/'generation-record3-failures.json').write_text(json.dumps(failures[:20],indent=2));assert not failures,len(failures)
expected=sum(1 for r in rows for f in r['native']['functions']for n in f['graph']['nodes']if n.get('right',{}).get('derived'));closures=sum(a.get('closure_checks',0)for a in actual);assert expected==closures and closures>0
arg_data=(P/'record-argument-commands.jsonl').read_bytes();arg_rows=[json.loads(x)for x in arg_data.splitlines()];assert len(arg_rows)==5376
arg_actual=[json.loads(x)for x in subprocess.check_output([str(direct)],input=arg_data).splitlines()];assert len(arg_actual)==len(arg_rows)
arg_failures=[{'index':i,'native':n['native'],'rust':a}for i,(n,a)in enumerate(zip(arg_rows,arg_actual))if n['native']!=a]
(P/'record-argument-failures.json').write_text(json.dumps(arg_failures[:20],indent=2));assert not arg_failures,len(arg_failures)
errors=sum('error'in r['native']for r in arg_rows);assert errors==96
assert all(r['profile']['original_sha256']=='08945A69DD63A44CE877D696E1C27BE2753DE29AE6FC1F8E183D37AB0ADD9882'for r in rows+arg_rows)
assert before==hashes(),'sources changed during proof';assert binaries=={str(p):sha(p)for p in [binary,direct]}
report={'whole_functions':len(rows),'exact_source_states':len(actual),'closure_captures':closures,'derived_operations':len(arg_rows),'native_errors':errors,'checked':['is_record/3 native [Dst, State, Tag, Arity] and [Args] closure environments','nested raw captured source state and function domains','prepared original declared/missing record tables and cached/uncached lookup','positive/false/unknown/none/unit destinations','singleton, multiple, unknown, non-atom, none and unit tags','negative, zero, singleton, finite-multiple, range, unbounded, float, none, unit and any arities','typed fields including impossible record prototype','1024/1025 tuple threshold','discarded lookup cache update during derived evaluation'],'remaining':['arity materialization above explicit 1000000 budget','whole upstream SCC execution and outer module fixpoint'],'full_core_generation':False,'source_sha256':before,'binary_sha256':binaries,'input_sha256':{'whole':hashlib.sha256(data).hexdigest(),'derived':hashlib.sha256(arg_data).hexdigest()}}
(P/'generation-record3-verification.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k]for k in ['whole_functions','closure_captures','derived_operations','native_errors']}))
