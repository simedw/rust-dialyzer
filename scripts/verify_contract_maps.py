#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/contract_maps';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/p for p in ['src/core.rs','src/bin/contract_map_probe.rs','scripts/core_export.exs','scripts/contract_map_oracle.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
files.append(Path(subprocess.check_output(['elixir','-e','IO.puts(:code.which(:maps))'],cwd=ROOT,text=True).strip()))
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files}
before=hashes();inputs={};count=0
subprocess.run(['cargo','build','--offline','--release','--bin','contract_map_probe'],cwd=ROOT,check=True)
for vm,mode in enumerate(['forward','reverse','reverse','forward']):
 native=OUT/f'{vm}.native.jsonl';rust=OUT/f'{vm}.rust.jsonl'
 with native.open('wb')as o:subprocess.run(['elixir','scripts/contract_map_oracle.exs',str(OUT/str(vm)),mode],cwd=ROOT,stdout=o,check=True)
 rows=[json.loads(s)for s in native.read_text().splitlines()];assert len(rows)==54
 inputs[str(native)]=digest(native)
 for row in rows:inputs[row['core_json']]=digest(Path(row['core_json']))
 with native.open('rb')as i,rust.open('wb')as o:subprocess.run([str(ROOT/'target/release/contract_map_probe')],cwd=ROOT,stdin=i,stdout=o,check=True)
 results=[json.loads(s)for s in rust.read_text().splitlines()];assert len(results)==54
 for row,result in zip(rows,results):assert result=={k:v for k,v in row.items()if k!='core_json'},(row,result)
 count+=len(rows)
assert before==hashes();assert all(digest(Path(p))==h for p,h in inputs.items())
report={'outcomes':count,'fresh_vms':4,'mismatches':0,'complete_compiler_variable_map_transport':True,'complete_contract_preparation':False,'implementation_sha256':before,'inputs_sha256':inputs}
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items()if not k.endswith('sha256')}))
