#!/usr/bin/env python3
"""Original succ_typings orchestration with declared phase fixtures."""
import argparse,hashlib,itertools,json,os,pathlib,subprocess,time,collections
sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--root',required=True);p.add_argument('--candidate-root',required=True);p.add_argument('--probe',required=True);p.add_argument('--out',required=True);a=p.parse_args();root=pathlib.Path(a.root).resolve();candidate=pathlib.Path(a.candidate_root).resolve();out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True);here=pathlib.Path(__file__).resolve().parent
 env=dict(os.environ,ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28',ASDF_RUST_VERSION='1.92.0');inputs=list((candidate/'src').rglob('*.rs'))+[candidate/f for f in ['Cargo.toml','Cargo.lock','corpus/bifs.json','corpus/dependency_policy.json']]+[root/f for f in ['scripts/type_codec.exs','scripts/type_wire.exs','scripts/core_export.exs']]+[here/'native_core_loop.exs',pathlib.Path(__file__).resolve()]
 beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_succ_typings,:dialyzer_callgraph,:dialyzer_plt,:dialyzer_codeserver,:dialyzer_contracts,:erl_types,:erl_bif_types,:sofs,:cerl,:cerl_trees],fn m->IO.puts(:code.which(m))end)'],cwd=root,env=env,text=True).splitlines();inputs.extend(map(pathlib.Path,beams));sources={str(f.resolve()):sha(f) for f in inputs};probe=pathlib.Path(a.probe).resolve();binary=sha(probe);start=time.monotonic();native=out/'native.jsonl';rust=out/'rust.jsonl'
 subprocess.run(['elixir',str(here/'native_core_loop.exs'),str(root),str(native)],cwd=root,env=env,check=True);native_hash=sha(native)
 with native.open('rb') as i,rust.open('wb') as o:subprocess.run([str(probe)],stdin=i,stdout=o,cwd=candidate,env=env,check=True)
 count=0;failures=[];events=collections.Counter();sparse=0
 with native.open() as n,rust.open() as r:
  for x,y in itertools.zip_longest(n,r):
   if x is None or y is None:raise ValueError('row count differs')
   expected=json.loads(x);actual=json.loads(y);count+=1;sparse+=expected['named_only']
   if expected['id']!=actual.get('id') or expected['native']!=actual.get('native') or expected['events']!=actual.get('events'):failures.append({'expected':expected['id'],'native':expected['native'],'events':expected['events'],'rust':actual})
   events.update(e['kind'] for e in expected['events'])
 if count!=216 or sparse!=108 or failures:raise ValueError({'cases':count,'sparse':sparse,'failures':len(failures),'first':failures[:3]})
 for f,h in sources.items():
  if sha(f)!=h:raise ValueError('source changed: '+f)
 if sha(native)!=native_hash or sha(probe)!=binary:raise ValueError('input or binary changed')
 report=dict(cases=count,matches=count,sparse_scc_cases=sparse,events=dict(events),source_sha256=sources,probe_sha256=binary,artifact_sha256={f.name:sha(f) for f in [native,rust]},elapsed_seconds=time.monotonic()-start,scope=dict(original_source_outer_loop=True,original_source_scc_publication=True,original_source_module_publication=True,original_contract_validation=True,declared_phase_outputs=True,native_inferred_input=False,actual_inference=False,actual_schedule_order=False,runtime_capture_parity=False))
 (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');print({k:v for k,v in report.items() if not k.endswith('sha256')})
if __name__=='__main__':main()
