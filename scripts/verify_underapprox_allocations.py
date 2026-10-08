#!/usr/bin/env python3
import hashlib,json,os,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/underapprox_allocations';OUT.mkdir(parents=True,exist_ok=True)
files=list((ROOT/'src').rglob('*.rs'))+[ROOT/p for p in ('Cargo.toml','Cargo.lock','scripts/generation_probes/native_underapprox_allocations.exs','scripts/core_export.exs','scripts/type_wire.exs','scripts/type_codec.exs')]+[Path(__file__).resolve()]
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files}
before=hashes();subprocess.run(['cargo','build','--release','--offline','--bin','underapprox_probe'],cwd=ROOT,check=True)
commands=OUT/'commands.jsonl';subprocess.run(['elixir',str(ROOT/'scripts/generation_probes/native_underapprox_allocations.exs'),str(ROOT),str(commands)],cwd=ROOT,check=True,env=os.environ|{'ASDF_ERLANG_VERSION':'28.3','ASDF_ELIXIR_VERSION':'1.19.4-otp-28'})
data=commands.read_bytes();input_hash=digest(commands);rows=[json.loads(s)for s in data.splitlines()];assert len(rows)==2560
binary=ROOT/'target/release/underapprox_probe';binary_hash=digest(binary);outputs=[json.loads(s)for s in subprocess.check_output([str(binary)],input=data).splitlines()]
assert len(outputs)==len(rows)and all(a['native']==b for a,b in zip(rows,outputs))
allocations=sum(r['native']['refold_allocations']for r in rows);assert allocations==276
assert before==hashes()and digest(commands)==input_hash and digest(binary)==binary_hash
report={'cases':2560,'matched':2560,'native_refold_allocations':276,'complete_captured_refold_allocation_order':True,'full_core_generation':False,'implementation_sha256':before,'inputs_sha256':{str(commands):input_hash,str(binary):binary_hash}}
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k]for k in ('cases','matched','native_refold_allocations')}))
