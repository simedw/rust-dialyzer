#!/usr/bin/env python3
import hashlib,json,shutil,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/guard_ops';S=OUT/'scripts';S.mkdir(parents=True,exist_ok=True)
files=list((ROOT/'src').rglob('*.rs'))+list((ROOT/'scripts/guard_probes').glob('*'))+[ROOT/p for p in ['scripts/type_codec.exs','scripts/type_wire.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
for name in ['erl_types','dialyzer_dataflow']:files.append(Path(subprocess.check_output(['elixir','-e',f'IO.puts(:code.which(:{name}))'],cwd=ROOT,text=True).strip()))
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files}
before=hashes()
(OUT/'data').mkdir(exist_ok=True)
for name in ['generate_guard_ops.py','guard_ops_oracle.exs','verify_guard_ops.py']:shutil.copyfile(ROOT/'scripts/guard_probes'/name,S/name)
for name in ['type_codec.exs','type_wire.exs']:shutil.copyfile(ROOT/'scripts'/name,S/name)
subprocess.run(['cargo','build','--offline','--release','--bin','guard_ops_probe'],cwd=ROOT,check=True)
subprocess.run([sys.executable,str(S/'generate_guard_ops.py')],cwd=OUT,check=True)
D=OUT/'data';commands=D/'guard_ops_cases.jsonl';inputs={str(commands):digest(commands)}
with commands.open('rb')as i,(D/'guard_ops_native.jsonl').open('wb')as o:subprocess.run(['elixir',str(S/'guard_ops_oracle.exs')],cwd=OUT,stdin=i,stdout=o,check=True)
with commands.open('rb')as i,(D/'guard_ops_rust.jsonl').open('wb')as o:subprocess.run([str(ROOT/'target/release/guard_ops_probe')],cwd=OUT,stdin=i,stdout=o,check=True)
subprocess.run([sys.executable,str(S/'verify_guard_ops.py')],cwd=OUT,check=True)
assert before==hashes();assert all(digest(Path(p))==h for p,h in inputs.items())
report=json.loads((D/'guard_ops_report.json').read_text());assert report['cases']==19020 and report['native_errors']==49
report.update(complete_guard_primitives=True,complete_guard_traversal=False,implementation_sha256=before,inputs_sha256=inputs)
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
