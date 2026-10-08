#!/usr/bin/env python3
"""Compare actual arena values and exact malformed-input errors through both libraries."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--baseline',type=Path,required=True)
p.add_argument('--candidate',type=Path,required=True)
p.add_argument('--fixture',type=Path,required=True)
p.add_argument('--out',type=Path,required=True)
a=p.parse_args()
a.out.mkdir(parents=True,exist_ok=False)
source=Path(__file__).with_name('arena_decode_probe.rs')
fixture=json.loads(a.fixture.read_text())
inputs=[fixture[key] for key in ['raw_summaries','raw_contracts','raw_callback_groups','type_tables','exported_types']]
scalars=[{'kind':'nil'},{'kind':'atom','value':'ok'},{'kind':'integer','value':'-99999999999999999999999999999'},{'kind':'float','value':'8000000000000000'},{'kind':'binary','value':'AQI='},{'kind':'bitstring','bytes':'oA==','bits':3}]
valid={'root':6,'terms':scalars+[{'kind':'tuple','items':list(range(6))}]}
inputs.append(valid)
inputs.extend([None,[],{}, {'root':0,'terms':[]}, {'root':0,'terms':None}])
mutations=[None,{}, {'kind':None},{'kind':'unknown'}, {'kind':'atom'},{'kind':'atom','value':1}, {'kind':'integer','value':'01'}, {'kind':'integer','value':'-0'}, {'kind':'float','value':'000000000000000a'}, {'kind':'float','value':1}, {'kind':'binary','value':'A==='}, {'kind':'binary','value':'Zh=='}, {'kind':'binary','value':'AA==AAAA'}, {'kind':'bitstring','bytes':'gQ==','bits':1}, {'kind':'bitstring','bytes':'gA==','bits':9}, {'kind':'bitstring','bytes':'gA==','bits':-1}, {'kind':'tuple','items':None}, {'kind':'tuple','items':[1]}, {'kind':'tuple','items':[-1]}, {'kind':'list','items':[],'tail':1}, {'kind':'list','items':[True],'tail':0}, {'kind':'map','entries':[[0]]}, {'kind':'map','entries':[[0,1]]}, {'kind':'map','entries':None}]
for mutation in mutations:
 inputs.append({'root':1,'terms':[{'kind':'nil'},mutation]})
for root in [None,-1,True,1.0,100]:
 case=copy.deepcopy(valid);case['root']=root;inputs.append(case)
data=('\n'.join(json.dumps(x) for x in inputs)+'\n').encode()
(a.out/'inputs.jsonl').write_bytes(data)
paths=[source,Path(__file__),a.fixture]
outputs=[]
for label,root in [('baseline',a.baseline),('candidate',a.candidate)]:
 deps=root/'target/release/deps'
 library=list(deps.glob('librust_dialyzer_poc-*.rlib'));serde=list(deps.glob('libserde_json-*.rlib'))
 assert len(library)==len(serde)==1
 paths.extend(library+serde)
 binary=a.out/label
 subprocess.run(['rustc','--edition=2024',str(source),'-L','dependency='+str(deps),'--extern','rust_dialyzer_poc='+str(library[0]),'--extern','serde_json='+str(serde[0]),'-o',str(binary)],check=True)
 run=subprocess.run([str(binary)],input=data,capture_output=True,check=True)
 (a.out/(label+'.jsonl')).write_bytes(run.stdout)
 outputs.append(run.stdout)
assert outputs[0]==outputs[1],'arena contents or exact errors differ'
rows=[json.loads(x) for x in outputs[0].splitlines()]
report=dict(cases=len(rows),valid=sum('error' not in r for r in rows),errors=sum('error' in r for r in rows),decoded_arenas_exact=True,error_strings_exact=True,input_sha256=hashlib.sha256(data).hexdigest(),provenance={str(x):hashlib.sha256(x.read_bytes()).hexdigest() for x in paths})
(a.out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
print({k:v for k,v in report.items() if k!='provenance'})
