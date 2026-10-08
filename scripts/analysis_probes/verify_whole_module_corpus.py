#!/usr/bin/env python3
"""Replay complete original module results through the shared Rust phase adapter."""
import argparse,hashlib,itertools,json,pathlib,subprocess,time
sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--root',required=True);p.add_argument('--proof',required=True);p.add_argument('--probe',required=True);p.add_argument('--out',required=True);a=p.parse_args();root=pathlib.Path(a.root).resolve();proof=pathlib.Path(a.proof).resolve();binary=pathlib.Path(a.probe).resolve();out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True)
 manifest_sha=sha(proof/'inputs.json');native_report_sha=sha(proof/'native_report.json');frozen=json.loads((proof/'inputs.json').read_text())
 for f,h in frozen.items():
  if sha(f)!=h:raise ValueError('original proof input changed: '+f)
 sources=list((root/'src').rglob('*.rs'))+[root/f for f in ['Cargo.toml','Cargo.lock','corpus/bifs.json','corpus/dependency_policy.json']]+[pathlib.Path(__file__).resolve()];before={str(f):sha(f) for f in sources};binary_before=sha(binary);start=time.monotonic();native=proof/'native.jsonl';commands=proof/'groups.jsonl';library=next(f for f in frozen if '/.cache/dependencies/base-' in f);profile=next(f for f in frozen if f.endswith('/.cache/analysis-profile.json'));actual=out/'rust.jsonl'
 with commands.open('rb') as i,actual.open('wb') as o:subprocess.run([str(binary),library,profile],stdin=i,stdout=o,cwd=root,check=True)
 count=events=0;failures=[]
 with native.open() as n,actual.open() as r:
  for x,y in itertools.zip_longest(n,r):
   if x is None or y is None:raise ValueError('module row count differs')
   expected=json.loads(x);got=json.loads(y);count+=1;events+=expected['native']['warning_count']
   if expected['id']!=got.get('id') or expected['native']!=got.get('native'):failures.append({'id':expected['id'],'native':expected['native'],'rust':got})
 if count!=571 or failures:raise ValueError({'modules':count,'failures':len(failures),'first':failures[:3]})
 for f,h in frozen.items():
  if sha(f)!=h:raise ValueError('original proof input changed: '+f)
 if sha(proof/'inputs.json')!=manifest_sha or sha(proof/'native_report.json')!=native_report_sha:raise ValueError('original proof metadata changed')
 if before!={str(f):sha(f) for f in sources} or sha(binary)!=binary_before:raise ValueError('candidate source or binary changed')
 report=dict(groups=327,modules=count,matches=count,raw_warning_events=events,source_sha256=before,binary_sha256=binary_before,original_proof_manifest_sha256=sha(proof/'inputs.json'),native_report_sha256=sha(proof/'native_report.json'),output_sha256=sha(actual),elapsed_seconds=time.monotonic()-start,scope=dict(actual_complete_module_refinement=True,actual_warning_pass_types=True,ordered_raw_warnings=True,independent_prepared_records=True,baseline_library_types_and_contracts=True,absent_prior_target_signatures=True,target_contracts_inactive=True,native_inferred_target_input=False,scheduler=False,runtime_capture_parity=False))
 (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');print({k:v for k,v in report.items() if not k.endswith('sha256')})
if __name__=='__main__':main()
