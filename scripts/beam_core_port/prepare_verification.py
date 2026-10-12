#!/usr/bin/env python3
"""Produce native stage files and an exact oracle-comparison plan."""
import argparse,json,pathlib,subprocess
p=argparse.ArgumentParser();p.add_argument('binary',type=pathlib.Path);p.add_argument('destination',type=pathlib.Path);p.add_argument('manifests',type=pathlib.Path,nargs='+');p.add_argument('--stages',nargs='+',choices=['forms','core','clean'],default=['forms','core','clean']);a=p.parse_args()
a.destination.mkdir(parents=True,exist_ok=True)
beams=sorted(set(beam for manifest in a.manifests for beam in manifest.read_text().splitlines()))
rows=[];plan=[]
for index,beam in enumerate(beams):
 for stage in a.stages:
  output=a.destination/f'{index}.{stage}.etf'
  run=subprocess.run([str(a.binary),stage,beam,str(output)],capture_output=True,text=True,timeout=60)
  row={'beam':beam,'stage':stage,'exit_code':run.returncode,'native_output':str(output)}
  if run.returncode: row['error']=run.stderr.strip()
  else: row['metrics']=json.loads(run.stdout);plan.append('\t'.join([beam,stage,str(output)]))
  rows.append(row)
(a.destination/'native-results.json').write_text(json.dumps(rows,indent=2)+'\n')
(a.destination/'comparison-plan.tsv').write_text('\n'.join(plan)+'\n')
print(json.dumps({'unique_beams':len(beams),'outputs':len(plan),'failures':len(rows)-len(plan)}))
