import hashlib,json,os,subprocess
from pathlib import Path
R=Path(__file__).resolve().parents[1];P=R/'.cache/generation_phase';P.mkdir(parents=True,exist_ok=True)
binary=R/'target/release/generation_probe'
producers=[('native_generation_apply_targets.exs','generation-apply-targets-commands.jsonl',750),('native_generation_phase_contracts.exs','generation-phase-contracts-commands.jsonl',2550),('native_generation_real_library.exs','generation-real-library-commands.jsonl',180)]
files=list((R/'src').rglob('*.rs'))+[R/'Cargo.toml',R/'Cargo.lock',R/'corpus/bifs.json',R/'corpus/dependency_policy.json',Path(__file__)]+[R/'scripts/generation_probes'/p for p,_,_ in producers]+[R/'scripts'/x for x in ['core_export.exs','type_wire.exs','type_codec.exs']]+[R/'.cache/corpus/otp/base.plt']
files.extend(Path(x) for x in subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_typesig,:erl_types,:erl_bif_types,:dialyzer_contracts,:dialyzer_plt],fn m->IO.puts(:code.which(m))end)'],cwd=R,text=True).splitlines())
def hashes():return {str(p):hashlib.sha256(p.read_bytes()).hexdigest()for p in files}
before=hashes();subprocess.run(['cargo','build','--offline','--release','--bin','generation_probe'],cwd=R,check=True);binary_hash=hashlib.sha256(binary.read_bytes()).hexdigest()
env=os.environ|{'ASDF_ERLANG_VERSION':'28.3','ASDF_ELIXIR_VERSION':'1.19.4-otp-28'}
rows=[];source_inputs={}
for producer,output,count in producers:
 subprocess.run(['elixir',str(R/'scripts/generation_probes'/producer),str(R),str(P/output)],check=True,env=env,cwd=R)
 data=(P/output).read_bytes();batch=[json.loads(x)for x in data.splitlines()];assert len(batch)==count,'case count changed';rows.extend(batch);source_inputs[output]=hashlib.sha256(data).hexdigest()
data=''.join(json.dumps(r)+'\n'for r in rows).encode();actual=[json.loads(x)for x in subprocess.check_output([str(binary)],input=data).splitlines()];assert len(actual)==len(rows)==3480,'partial output'
def norm(v):
 if isinstance(v,dict):
  if 'derived'in v:return {'derived':True}
  return {k:norm(x)for k,x in v.items()if k!='closure_checks'}
 if isinstance(v,list):return [norm(x)for x in v]
 return v
failures=[{'index':i,'native':n['native'],'rust':a}for i,(n,a)in enumerate(zip(rows,actual))if norm(n['native'])!=norm(a)];(P/'generation-phase-failures.json').write_text(json.dumps(failures,indent=2));assert not failures,[(f['index'],f['rust'].get('error','graph/bindings'))for f in failures[:5]]
expected=sum(1 for r in rows for f in r['native']['functions']for n in f['graph']['nodes']if n.get('right',{}).get('derived'));closures=sum(a.get('closure_checks',0)for a in actual);assert expected==closures and closures>0,'missing closure checks'
assert all(r['profile']['original_sha256']=='08945A69DD63A44CE877D696E1C27BE2753DE29AE6FC1F8E183D37AB0ADD9882'for r in rows),'native profile changed'
base_sha=hashlib.sha256((R/'.cache/corpus/otp/base.plt').read_bytes()).hexdigest().upper()
real=[r for r in rows if 'base_plt_sha256'in r];assert len(real)==180 and all(r['base_plt_sha256']==base_sha for r in real),'base PLT identity changed'
assert before==hashes(),'sources changed during proof'
assert hashlib.sha256(binary.read_bytes()).hexdigest()==binary_hash
assert all(hashlib.sha256((P/name).read_bytes()).hexdigest()==digest for name,digest in source_inputs.items())
source_inputs[str(binary)]=binary_hash
report={'cases':len(rows),'matched':len(actual),'closure_captures':closures,'checked':['complete constraint graphs/dependencies','exact native source closure captures and ordering','fresh positive labels','exported native success bindings','processed contract generic arguments'],'scope':['source-derived Program and ClosureDependencies target sets','named apply-target PLT alternatives in native label order','external/unnamed apply targets preserve generic constraints','failure alternatives and nested closures','controlled prior PLT signature inputs','actual imported base PLT: 18 MFAs, 180 signature cases and 110 contract cases','controlled active processed contracts, bounded variables and overloads','signature-plus-contract and contract-only static calls'],'remaining':['record-sensitive is_record/3 raw state capture','production SCC phase orchestration and full upstream success parity'],'full_core_generation':False,'real_dependency_plt_sha256':base_sha,'source_sha256':before,'input_sha256':source_inputs}
(P/'generation-phase-verification.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k]for k in ['cases','matched','closure_captures','full_core_generation']}))
