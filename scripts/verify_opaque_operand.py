#!/usr/bin/env python3
import hashlib,json,shutil,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/opaque_operand';OUT.mkdir(parents=True,exist_ok=True)
files=list((ROOT/'src').rglob('*.rs'))+[ROOT/p for p in ['scripts/guard_probes/opaque_operand_oracle.exs','scripts/guard_probes/opaque_operand_deep_cases.py','scripts/guard_probes/opaque_operand_deep_oracle.exs','scripts/core_export.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
files.append(Path(subprocess.check_output(['elixir','-e','IO.puts(:code.which(:erl_types))'],cwd=ROOT,text=True).strip()))
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files}
before=hashes()
for source,name in [(ROOT/'scripts/core_export.exs','core_export.exs'),(ROOT/'scripts/guard_probes/opaque_operand_oracle.exs','oracle.exs')]:shutil.copyfile(source,OUT/name)
subprocess.run(['cargo','build','--offline','--release','--bin','opaque_operand_probe'],cwd=ROOT,check=True)
with (OUT/'commands.jsonl').open('wb')as o:subprocess.run(['elixir','oracle.exs'],cwd=OUT,stdout=o,check=True)
inputs={str(OUT/'commands.jsonl'):digest(OUT/'commands.jsonl')}
with (OUT/'commands.jsonl').open('rb')as i,(OUT/'rust.jsonl').open('wb')as o:subprocess.run([str(ROOT/'target/release/opaque_operand_probe')],cwd=ROOT,stdin=i,stdout=o,check=True)
commands=[json.loads(s)for s in (OUT/'commands.jsonl').read_text().splitlines()];rust=[json.loads(s)for s in (OUT/'rust.jsonl').read_text().splitlines()]
assert len(commands)==len(rust)==165;assert all(a['id']==b['id']and b['matches']is True for a,b in zip(commands,rust));assert before==hashes();assert all(digest(Path(p))==h for p,h in inputs.items())
for name in ['opaque_operand_deep_oracle.exs','opaque_operand_deep_cases.py']:shutil.copyfile(ROOT/'scripts/guard_probes'/name,OUT/name)
with (OUT/'deep_cases.jsonl').open('wb')as o:subprocess.run([sys.executable,'opaque_operand_deep_cases.py'],cwd=OUT,stdout=o,check=True)
inputs[str(OUT/'deep_cases.jsonl')]=digest(OUT/'deep_cases.jsonl')
with (OUT/'deep_cases.jsonl').open('rb')as i,(OUT/'deep_native.jsonl').open('wb')as o:subprocess.run(['elixir','opaque_operand_deep_oracle.exs'],cwd=OUT,stdin=i,stdout=o,check=True)
inputs[str(OUT/'deep_native.jsonl')]=digest(OUT/'deep_native.jsonl')
with (OUT/'deep_native.jsonl').open('rb')as i,(OUT/'deep_rust.jsonl').open('wb')as o:subprocess.run([str(ROOT/'target/release/opaque_operand_probe')],cwd=OUT,stdin=i,stdout=o,check=True)
deep=[json.loads(s)for s in (OUT/'deep_rust.jsonl').read_text().splitlines()];assert len(deep)==2 and all(row['matches']is True and row['output']=={'ok':False}for row in deep)
assert before==hashes();assert all(digest(Path(p))==h for p,h in inputs.items())
report={'outcomes':165,'deep_shared_outcomes':2,'mismatches':0,'complete_captured_opaque_inspection':True,'complete_guard_traversal':False,'implementation_sha256':before,'inputs_sha256':inputs}
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print('165 exact opaque operand outcomes')
