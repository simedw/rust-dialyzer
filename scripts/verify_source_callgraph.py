#!/usr/bin/env python3
"""Ordered callgraph phase proof; consumes same-VM compiler Core, never native inference."""
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/source_callgraph';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/p for p in ['src/source_callgraph.rs','src/closure_dependencies.rs','src/core.rs','src/labels.rs','src/program.rs','src/program_metadata.rs','src/dataflow_nodes.rs','src/native_dict.rs','src/worklist.rs','src/schedule.rs','src/bin/source_callgraph_probe.rs','corpus/bifs.json','corpus/dependency_policy.json','scripts/source_callgraph_oracle.exs','scripts/source_callgraph_cases.exs','scripts/core_export.exs','Cargo.toml','Cargo.lock']]+list((ROOT/'src/bifs').glob('*.rs'))+[Path(__file__).resolve()]
for name in ['dialyzer_callgraph','dialyzer_dep','erl_bif_types','dict','digraph','digraph_utils','sofs']:
 files.append(Path(subprocess.check_output(['elixir','-e',f'IO.puts(:code.which(:{name}))'],cwd=ROOT,text=True).strip()))
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files}
before=hashes();inputs={}
rows=json.loads((ROOT/'.cache/core/report.json').read_text())['cases'];assert len(rows)==327
synthetic=subprocess.check_output(['elixir','scripts/source_callgraph_cases.exs',str(OUT/'synthetic')],cwd=ROOT)
extra=[json.loads(s)for s in synthetic.splitlines()];assert len(extra)==20;rows+=extra
for r in rows:inputs[r['core_json']]=digest(Path(r['core_json']))
(OUT/'corpus.jsonl').write_text(''.join(json.dumps({'id':r['id'],'core_json':r['core_json']})+'\n'for r in rows))
subprocess.run(['cargo','build','--offline','--release','--bin','source_callgraph_probe'],cwd=ROOT,check=True)
with (OUT/'corpus.jsonl').open('rb')as i,(OUT/'native.jsonl').open('wb')as o:subprocess.run(['elixir','scripts/source_callgraph_oracle.exs'],cwd=ROOT,stdin=i,stdout=o,check=True)
native=[json.loads(s)for s in (OUT/'native.jsonl').read_text().splitlines()];assert len(native)==347
for row in native:inputs[row['proof_core_json']]=digest(Path(row['proof_core_json']))
inputs[str(OUT/'native.jsonl')]=digest(OUT/'native.jsonl')
with (OUT/'native.jsonl').open('rb')as i,(OUT/'rust.jsonl').open('wb')as o:subprocess.run([str(ROOT/'target/release/source_callgraph_probe')],cwd=ROOT,stdin=i,stdout=o,check=True)
rust=[json.loads(s)for s in (OUT/'rust.jsonl').read_text().splitlines()];assert len(rust)==347
fail=[]
for n,r in zip(native,rust):
 expected={k:v for k,v in n.items()if k!='proof_core_json'}
 if expected!=r:fail.append({'id':n['id'],'expected':expected,'actual':r})
(OUT/'differences.json').write_text(json.dumps(fail,indent=2)+'\n')
assert before==hashes(),'source changed during proof';assert all(digest(Path(p))==h for p,h in inputs.items()),'input changed during proof'
report={'cases':327,'synthetic_cases':20,'synthetic_errors':sum('error'in n for n in native[327:]),'modules':sum(len(n['modules'])for n in native[:327]),'synthetic_modules':16,'mismatches':len(fail),'complete_ordered_source_edges':not fail,'complete_native_scc_order':False,'complete_scc_partition':not fail,'components':sum(len(n.get('components',[]))for n in native),'implementation_sha256':before,'inputs_sha256':inputs}
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items()if not k.endswith('sha256')}))
assert not fail,f'{len(fail)} differing cases, see {OUT}/differences.json'
