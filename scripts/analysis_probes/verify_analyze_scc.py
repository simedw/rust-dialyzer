#!/usr/bin/env python3
import hashlib,itertools,json,subprocess,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];HERE=Path(__file__).resolve().parent;OUT=ROOT/'.cache/analyze_scc';OUT.mkdir(parents=True,exist_ok=True)
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
files=list((ROOT/'src').rglob('*.rs'))+[ROOT/p for p in ['Cargo.toml','Cargo.lock','corpus/bifs.json','corpus/dependency_policy.json','scripts/core_export.exs','scripts/type_wire.exs','scripts/type_codec.exs']]+[HERE/p for p in ['native_analyze_scc.exs','source_capture_wire.exs','verify_analyze_scc.py']]
beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_typesig,:dialyzer_callgraph,:dialyzer_plt,:dialyzer_codeserver,:dialyzer_utils,:erl_types,:erl_bif_types,:cerl,:cerl_trees],fn m->IO.puts(:code.which(m))end)'],cwd=ROOT,text=True).splitlines();files.extend(map(Path,beams));sources={str(p):sha(p) for p in files};binary=ROOT/'target/release/generation_probe';binary_sha=sha(binary);start=time.monotonic()
commands=OUT/'native.jsonl';actual=OUT/'rust.jsonl';subprocess.run(['elixir',str(HERE/'native_analyze_scc.exs'),str(ROOT),str(commands)],cwd=ROOT,check=True);input_sha=sha(commands)
with commands.open('rb') as i,actual.open('wb') as o:subprocess.run([str(binary)],cwd=ROOT,stdin=i,stdout=o,check=True)
count=errors=captures=0;failures=[]
def norm(v):
 if isinstance(v,dict):return {k:norm(x) for k,x in v.items() if k!='source_state_matched'}
 if isinstance(v,list):return [norm(x) for x in v]
 return v
with commands.open() as n,actual.open() as r:
 for x,y in itertools.zip_longest(n,r):
  assert x is not None and y is not None
  expected=json.loads(x);result=json.loads(y);count+=1
  if norm(expected['native'])!=norm(result):failures.append({'index':count,'native':expected['native'],'rust':result})
  errors+=expected['native']['status']=='native_error';captures+=result.get('source_state_matched') is True
(OUT/'failures.json').write_text(json.dumps(failures[:20],indent=2)+'\n');assert count==1512 and errors==252 and captures==1260 and not failures,(count,errors,captures,len(failures))
assert sources=={str(p):sha(p) for p in files};assert sha(binary)==binary_sha and sha(commands)==input_sha
report=dict(cases=count,matches=count,native_errors=errors,exact_source_states=captures,source_sha256=sources,binary_sha256=binary_sha,input_sha256=input_sha,output_sha256=sha(actual),elapsed_seconds=time.monotonic()-start,remaining=['complete outer module/SCC convergence'])
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print({k:report[k] for k in ['cases','matches','native_errors','exact_source_states','elapsed_seconds']})
