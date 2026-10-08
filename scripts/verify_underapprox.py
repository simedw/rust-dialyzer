import hashlib,json,os,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];HERE=ROOT/'.cache/underapprox';HERE.mkdir(parents=True,exist_ok=True)
files=list((ROOT/'src').rglob('*.rs'))+[ROOT/p for p in ['Cargo.toml','Cargo.lock','scripts/generation_probes/native_underapprox.exs','scripts/generation_probes/native_underapprox_corpus.exs','scripts/verify_underapprox.py','scripts/core_export.exs','scripts/type_codec.exs','scripts/type_wire.exs']]
def hashes():return {str(p):hashlib.sha256(p.read_bytes()).hexdigest()for p in files}
before=hashes();binary=ROOT/'target/release/underapprox_probe'
subprocess.run(['cargo','build','--release','--offline','--bin','underapprox_probe'],cwd=ROOT,check=True)
env=os.environ|{'ASDF_ERLANG_VERSION':'28.3','ASDF_ELIXIR_VERSION':'1.19.4-otp-28'}
subprocess.run(['elixir',str(ROOT/'scripts/generation_probes/native_underapprox.exs'),str(ROOT),str(HERE/'underapprox-commands.jsonl')],check=True,env=env)
synthetic=(HERE/'underapprox-commands.jsonl').read_bytes();rows=[json.loads(x)for x in synthetic.splitlines()];assert len(rows)==2560,'changed synthetic coverage'
synthetic_hash=hashlib.sha256(synthetic).hexdigest()
results=[json.loads(x)for x in subprocess.check_output([str(binary)],input=synthetic).splitlines()];assert len(results)==len(rows),'partial synthetic output'
failures=[{'index':i,'native':r['native'],'rust':s}for i,(r,s)in enumerate(zip(rows,results))if r['native']!=s];assert not failures,json.dumps(failures[:3])
cases=json.loads((ROOT/'.cache/core/report.json').read_text());assert cases['complete'] and len(cases['cases'])==327,'incomplete compiler corpus'
inputs=[{'id':c['id'],'core_json':c['core_json']}for c in cases['cases']];core_hashes={c['core_json']:hashlib.sha256(Path(c['core_json']).read_bytes()).hexdigest()for c in inputs}
commands=''.join(json.dumps(c)+'\n'for c in inputs).encode();native=[json.loads(x)for x in subprocess.check_output(['elixir',str(ROOT/'scripts/generation_probes/native_underapprox_corpus.exs'),str(ROOT)],input=commands,env=env).splitlines()];rust=[json.loads(x)for x in subprocess.check_output([str(binary)],input=commands).splitlines()]
assert len(native)==len(rust)==len(inputs),'partial corpus output'
assert [n['id']for n in native]==[r['id']for r in rust]==[c['id']for c in inputs],'missing/reordered corpus group'
clauses=sum(len(n['clauses'])for n in native);assert clauses==59189,'changed clause coverage'
failures=[{'id':c['id'],'native':n,'rust':r}for c,n,r in zip(inputs,native,rust)if n!=r];assert not failures,json.dumps(failures[:1])[:8000]
assert hashlib.sha256((HERE/'underapprox-commands.jsonl').read_bytes()).hexdigest()==synthetic_hash
assert before==hashes(),'source changed during proof'
assert core_hashes=={p:hashlib.sha256(Path(p).read_bytes()).hexdigest()for p in core_hashes},'compiler corpus changed during proof'
report={'synthetic_cases':len(rows),'synthetic_native_errors':sum('error'in r['native']for r in rows),'compiler_groups':len(inputs),'native_clauses':clauses,'matched_clauses':clauses,'matched_synthetics':len(rows),'implementation_sha256':before,'inputs_sha256':core_hashes|{str(HERE/'underapprox-commands.jsonl'):synthetic_hash},'complete_source_underapprox':True,'full_analyzer_parity':False,'resource_boundary':'explicit depth128/one million work items before recursive Type ownership; never substitutes Any/None for exhausted resources'}
(HERE/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k]for k in ['synthetic_cases','synthetic_native_errors','compiler_groups','native_clauses','matched_clauses','full_analyzer_parity']}))
