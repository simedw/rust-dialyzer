#!/usr/bin/env python3
"""Fresh independent Rust lifecycle and canonical graph gate; Native output is expected-only."""
import argparse,hashlib,json,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--out',type=Path,required=True)
for name in ['native','synthetic-native','graph-native','helper-native']:p.add_argument('--'+name,type=Path,required=True)
a=p.parse_args();a.out.mkdir(parents=True,exist_ok=True)
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
inputs={}
for file in list((a.root/'src').rglob('*.rs'))+[a.root/'Cargo.toml',a.root/'Cargo.lock',a.root/'corpus/bifs.json',a.root/'corpus/dependency_policy.json',Path(__file__).resolve()]:inputs[str(file.resolve())]=digest(file)
for file in Path(__file__).resolve().parent.glob('source_*oracle.exs'):inputs[str(file.resolve())]=digest(file)
for file in Path(__file__).resolve().parent.glob('source_*cases.exs'):inputs[str(file.resolve())]=digest(file)
for attr in ['native','synthetic_native','graph_native','helper_native']:
 file=getattr(a,attr);inputs[str(file.resolve())]=digest(file)
 if attr!='graph_native':
  for line in file.open():
   row=json.loads(line);core=Path(row['proof_core_json']);inputs[str(core.resolve())]=digest(core)
for name in ['source_schedule_probe','source_graph_probe','source_callgraph_probe']:
 file=a.root/'target/release'/name;inputs[str(file.resolve())]=digest(file)
(a.out/'input_hashes.json').write_text(json.dumps(inputs,indent=2)+'\n')
summary={}
for attr,bin_name,expected_count in [('native','source_schedule_probe',347),('synthetic_native','source_schedule_probe',54),('graph_native','source_graph_probe',90),('helper_native','source_callgraph_probe',347)]:
 source=getattr(a,attr);target=a.out/(attr+'_rust.jsonl')
 with source.open('rb') as i,target.open('wb') as o:subprocess.run([str(a.root/'target/release'/bin_name)],stdin=i,stdout=o,check=True,cwd=a.root)
 native=list(map(json.loads,source.open()));rust=list(map(json.loads,target.open()));assert len(native)==len(rust)==expected_count,(attr,len(native),len(rust))
 failures=[];errors=0
 for n,r in zip(native,rust):
  if attr=='helper_native' or 'error' in n:
   matched={k:v for k,v in n.items() if k!='proof_core_json'}==r
   errors+='error' in n
  else:matched=r.get('matches') is True and n['id']==r.get('id')
  if not matched:failures.append({'id':n['id'],'actual':r})
 (a.out/(attr+'_differences.json')).write_text(json.dumps(failures,indent=2)+'\n')
 summary[attr]={'cases':len(native),'original_errors':errors,'steps':sum(len(n.get('steps',[])) for n in native),'mismatches':len(failures)}
 assert not failures,(attr,len(failures))
assert all(digest(Path(file))==h for file,h in inputs.items()),'Source or proof inputs changed during final replay'
summary['freshness']=True
(a.out/'report.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary))
