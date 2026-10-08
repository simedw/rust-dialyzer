#!/usr/bin/env python3
"""Compare original state__get_warnings with the canonical BodyFlow controller."""
import argparse,hashlib,itertools,json,os,pathlib,subprocess,time
sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--root',required=True);p.add_argument('--candidate-root',required=True);p.add_argument('--probe',required=True);p.add_argument('--out',required=True);a=p.parse_args()
 root=pathlib.Path(a.root).resolve();candidate=pathlib.Path(a.candidate_root).resolve();out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True);here=pathlib.Path(__file__).resolve().parent
 env=dict(os.environ,ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28',ASDF_RUST_VERSION='1.92.0')
 inputs=list((candidate/'src').rglob('*.rs'))+[candidate/f for f in ['Cargo.toml','Cargo.lock','corpus/bifs.json','corpus/dependency_policy.json']]+[root/f for f in ['scripts/type_codec.exs','scripts/type_wire.exs','scripts/core_export.exs','scripts/dataflow_state_probes/function_state_oracle.exs']]+[here/'native_function_warnings.exs',pathlib.Path(__file__).resolve()]
 beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_dataflow,:dialyzer_callgraph,:dialyzer_dep,:dialyzer_plt,:dialyzer_codeserver,:dialyzer_contracts,:erl_types,:erl_bif_types,:cerl,:cerl_trees,:dict,:sets],fn m->IO.puts(:code.which(m))end)'],cwd=root,env=env,text=True).splitlines();inputs.extend(map(pathlib.Path,beams));sources={str(f.resolve()):sha(f) for f in inputs};probe=pathlib.Path(a.probe).resolve();binary=sha(probe);start=time.monotonic()
 native=out/'native.jsonl';rust=out/'rust.jsonl';subprocess.run(['elixir',str(here/'native_function_warnings.exs'),str(root),str(native)],cwd=root,env=env,check=True);native_hash=sha(native)
 with native.open('rb') as i,rust.open('wb') as o:subprocess.run([str(probe)],stdin=i,stdout=o,cwd=candidate,env=env,check=True)
 count=events=warn_rows=baseline_rows=0;failures=[];graph_counts={}
 with native.open() as n,rust.open() as r:
  for x,y in itertools.zip_longest(n,r):
   if x is None or y is None:raise ValueError('row count differs')
   expected=json.loads(x);actual=json.loads(y);count+=1
   if expected['id']!=actual.get('id') or expected['native']!=actual.get('native'):failures.append({'expected':expected['id'],'native':expected['native'],'rust':actual})
   
   for fact in expected['graph_evidence']:
    key=f"escaping={fact['escaping']},incoming={fact['incoming']},parent_named={fact['parent_named']}";graph_counts[key]=graph_counts.get(key,0)+1
   events+=expected['native']['warning_count'];warn_rows+=expected['native']['warning_count']>0;baseline_rows+=expected['baseline_node'] is not None
 if count!=12000 or baseline_rows!=6000 or failures:raise ValueError({'cases':count,'failures':len(failures),'first':failures[:8]})
 for f,h in sources.items():
  if sha(f)!=h:raise ValueError('source changed: '+f)
 if sha(native)!=native_hash or sha(probe)!=binary:raise ValueError('input or binary changed')
 report=dict(cases=count,matches=count,warning_rows=warn_rows,warning_events=events,baseline_rows=baseline_rows,anonymous_graph_evidence=graph_counts,source_sha256=sources,probe_sha256=binary,artifact_sha256={f.name:sha(f) for f in [native,rust]},elapsed_seconds=time.monotonic()-start,scope=dict(original_source_state_get_warnings=True,raw_warning_order_and_payload=True,source_core_graph_facts=True,explicit_prior_phase_operations=True,current_fun_mutation=False,module_inference=False,malformed_core=False))
 (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');print({k:v for k,v in report.items() if not k.endswith('sha256')})
if __name__=='__main__':main()
