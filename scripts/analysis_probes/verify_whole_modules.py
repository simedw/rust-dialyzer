#!/usr/bin/env python3
"""Exact original get_fun_types/get_warnings on complete compiler modules."""
import argparse,collections,hashlib,itertools,json,os,pathlib,subprocess,time
sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--root',required=True);p.add_argument('--candidate-root',required=True);p.add_argument('--probe',required=True);p.add_argument('--out',required=True);a=p.parse_args();root=pathlib.Path(a.root).resolve();candidate=pathlib.Path(a.candidate_root).resolve();here=pathlib.Path(__file__).resolve().parent;out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True)
 env=dict(os.environ,ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28',ASDF_RUST_VERSION='1.92.0');base=root/'.cache/corpus/otp/base.plt';library=root/'.cache/dependencies/base-573d6b6f76baf86026e6ed855a238266a2d36b89919a88ce5211465196227f56.json';profile=root/'.cache/analysis-profile.json'
 inputs=list((candidate/'src').rglob('*.rs'))+[candidate/f for f in ['Cargo.toml','Cargo.lock','corpus/bifs.json','corpus/dependency_policy.json']]+[root/f for f in ['scripts/type_codec.exs','scripts/type_wire.exs','scripts/core_export.exs']]+[here/'native_whole_modules.exs',pathlib.Path(__file__).resolve(),base,library,profile]
 beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_dataflow,:dialyzer_callgraph,:dialyzer_plt,:dialyzer_codeserver,:erl_types,:erl_bif_types,:cerl,:cerl_trees,:compile],fn m->IO.puts(:code.which(m))end)'],cwd=root,env=env,text=True).splitlines();inputs.extend(map(pathlib.Path,beams));sources={str(f.resolve()):sha(f) for f in inputs};probe=pathlib.Path(a.probe).resolve();binary=sha(probe);start=time.monotonic();native=out/'native.jsonl';rust=out/'rust.jsonl'
 subprocess.run(['elixir',str(here/'native_whole_modules.exs'),str(root),str(native),str(base)],cwd=root,env=env,check=True);native_hash=sha(native)
 with native.open('rb') as i,rust.open('wb') as o:subprocess.run([str(probe),str(library),str(profile)],stdin=i,stdout=o,cwd=candidate,env=env,check=True)
 count=warning_cases=warning_events=0;failures=[];profiles=collections.Counter()
 with native.open() as n,rust.open() as r:
  for x,y in itertools.zip_longest(n,r):
   if x is None or y is None:raise ValueError('row count differs')
   expected=json.loads(x);actual=json.loads(y);count+=1;profiles.update([expected['id'].split(':')[1]]);warning_events+=expected['native']['warning_count'];warning_cases+=expected['native']['warning_count']>0
   if expected['native']['status']!='ok' or expected['id']!=actual.get('id') or expected['native']!=actual.get('native'):failures.append({'id':expected['id'],'native':expected['native'],'rust':actual})
 if count!=60 or failures:raise ValueError({'cases':count,'failures':len(failures),'first':failures[:3]})
 for f,h in sources.items():
  if sha(f)!=h:raise ValueError('source changed: '+f)
 if sha(native)!=native_hash or sha(probe)!=binary:raise ValueError('input or binary changed')
 report=dict(cases=count,matches=count,warning_cases=warning_cases,warning_events=warning_events,prior_profiles=dict(profiles),source_sha256=sources,probe_sha256=binary,artifact_sha256={f.name:sha(f) for f in [native,rust]},compiler_artifact_sha256={str(f.relative_to(out)):sha(f) for f in (out/'fixtures').rglob('*') if f.is_file()},elapsed_seconds=time.monotonic()-start,scope=dict(original_get_fun_types=True,original_get_warnings=True,complete_compiler_modules=True,independent_prepared_definitions=True,actual_library_cache=True,declared_prior_signatures=True,native_inferred_target_input=False,full_raw_warning_order=True,records_and_contracts=False,runtime_capture_parity=False,outer_schedule=False))
 (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');print({k:v for k,v in report.items() if not k.endswith('sha256')})
if __name__=='__main__':main()
