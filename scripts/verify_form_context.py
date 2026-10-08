#!/usr/bin/env python3
import hashlib,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];SCRIPTS=ROOT/'scripts/form_export_probes';OUT=ROOT/'.cache/form_context';OUT.mkdir(parents=True,exist_ok=True)
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
files=[*sorted((ROOT/'src').glob('*.rs')),ROOT/'src/bin/cache_probe.rs',ROOT/'src/bin/form_io_probe.rs',*sorted(SCRIPTS.glob('*')),ROOT/'scripts/core_export.exs',ROOT/'Cargo.toml',ROOT/'Cargo.lock',Path(__file__).resolve()]
def hashes():return {str(p.relative_to(ROOT)):digest(p)for p in files if p.is_file()}
implementation=hashes()
subprocess.run(['cargo','build','--release','--offline','--bin','cache_probe','--bin','form_io_probe'],cwd=ROOT,check=True)
reports=[]
proof_inputs={}
for name,count in [('native_context_boundary',16),('record_context',10)]:
 subprocess.run([sys.executable,str(SCRIPTS/f'{name}_cases.py')],cwd=OUT,check=True)
 commands=OUT/f'{name}_cases.jsonl';proof_inputs[str(commands.relative_to(ROOT))]=digest(commands);rows=[json.loads(s)for s in commands.read_text().splitlines()]
 if len(rows)!=count or len({r['label']for r in rows})!=count:raise RuntimeError(f'{name} request identities changed')
 native=OUT/f'{name}.native.jsonl';rust=OUT/f'{name}.rust.jsonl'
 with native.open('w')as out:subprocess.run(['elixir',str(SCRIPTS/'native_context_boundary_reference.exs'),str(commands)],cwd=OUT,stdout=out,check=True)
 with commands.open()as inp,rust.open('w')as out:subprocess.run([str(ROOT/'target/release/cache_probe')],cwd=OUT,stdin=inp,stdout=out,check=True)
 subprocess.run(['elixir',str(SCRIPTS/'boundary_full_oracle.exs'),str(native),str(rust)],cwd=OUT,stdout=subprocess.DEVNULL,check=True)
 report=json.loads(Path(str(rust)+'.full_comparison.json').read_text())
 if report['cases']!=count or report['failures']:raise RuntimeError(f'{name} full outcome/effect proof failed')
 reports.append(dict(gate=name,**report))
subprocess.run([sys.executable,str(SCRIPTS/'form_io_cases.py')],cwd=OUT,check=True)
commands=OUT/'form_io_commands.jsonl';captured=OUT/'form_io_captured.jsonl';rust=OUT/'form_io_rust.jsonl'
subprocess.run(['elixir',str(SCRIPTS/'form_io_capture.exs'),str(commands),str(captured)],cwd=OUT,check=True)
proof_inputs[str(commands.relative_to(ROOT))]=digest(commands)
proof_inputs[str(captured.relative_to(ROOT))]=digest(captured)
if len(commands.read_text().splitlines())!=7253 or len(captured.read_text().splitlines())!=7253:raise RuntimeError('form writer denominator changed')
with captured.open()as inp,rust.open('w')as out:subprocess.run([str(ROOT/'target/release/form_io_probe')],cwd=OUT,stdin=inp,stdout=out,check=True)
subprocess.run(['elixir',str(SCRIPTS/'form_io_oracle.exs'),str(captured),str(rust)],cwd=OUT,stdout=subprocess.DEVNULL,check=True)
writer=json.loads(Path(str(rust)+'.comparison.json').read_text())
if writer['cases']!=7253 or writer['failures']:raise RuntimeError('form writer full iolist comparison failed')
allocation=[]
for _ in range(4):
 for order in ['ab','ba']:
  row=json.loads(subprocess.check_output(['elixir',str(SCRIPTS/'map_atom_order.exs'),order],cwd=OUT))
  if row['physical']!=row['allocation'] or row['iterator']!=row['allocation'] or row['to_list']!=row['allocation'] or row['ordered']!=sorted(row['allocation']):raise RuntimeError('VM physical-map allocation proof failed')
  allocation.append(row)
if proof_inputs!={name:digest(ROOT/name)for name in proof_inputs}:raise RuntimeError('proof input changed during verification')
if implementation!=hashes():raise RuntimeError('form context implementation changed during verification')
source=ROOT/'.cache/upstream/otp/lib/dialyzer/src/erl_types.erl'
inputs={str(source.relative_to(ROOT)):digest(source),**proof_inputs}
report=dict(commands=7279,matched=7279,complete_captured_context_boundary=True,complete_arbitrary_form_boundary=False,context=reports,writer=writer,allocation_probes=allocation,implementation_sha256=implementation,inputs_sha256=inputs,unsupported=['parser short-circuit timing on malformed cache hits and budgets','arbitrary grammar and metadata scopes','raw map formatting without iterator provenance','full analysis lifecycle'])
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k]for k in ['commands','matched','complete_arbitrary_form_boundary']}))
