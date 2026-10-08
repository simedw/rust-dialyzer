#!/usr/bin/env python3
"""Verify the shared module function state against the original OTP helpers."""
import hashlib,json,shutil,subprocess,sys
from contextlib import nullcontext
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.cache/function_state';OUT.mkdir(parents=True,exist_ok=True)
HELP=ROOT/'scripts/dataflow_state_probes'
FILES=list(ROOT.joinpath('src').rglob('*.rs'))+list(HELP.glob('*'))+[ROOT/p for p in ['Cargo.toml','Cargo.lock','scripts/core_export.exs','scripts/type_codec.exs','scripts/type_wire.exs']]+[Path(__file__).resolve()]
for name in ['dialyzer_dataflow','dialyzer_callgraph','dict','erl_types']:
 FILES.append(Path(subprocess.check_output(['elixir','-e',f'IO.puts(:code.which(:{name}))'],cwd=ROOT,text=True).strip()))
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p) for p in FILES if p.is_file()}
before=hashes();inputs={}
def retain(p):inputs[str(p)]=digest(p)
def run(cmd,source=None,target=None):
 if source:retain(source)
 with (source.open('rb') if source else open('/dev/null','rb'))as i,(target.open('wb') if target else nullcontext(sys.stdout.buffer))as o:
  subprocess.run(cmd,cwd=OUT,stdin=i,stdout=o,check=True)
for p in HELP.glob('*'):
 if p.is_file():shutil.copyfile(p,OUT/p.name)
for name in ['core_export.exs','type_codec.exs','type_wire.exs']:shutil.copyfile(ROOT/'scripts'/name,OUT/name)
D=OUT/'data';D.mkdir(exist_ok=True)
manifest=json.loads((ROOT/'.cache/core/report.json').read_text())['cases'];assert len(manifest)==327
for row in manifest:retain(Path(row['core_json']))
(D/'corpus_inputs.jsonl').write_text(''.join(json.dumps({'id':r['id'],'core_json':r['core_json']})+'\n' for r in manifest))
subprocess.run(['cargo','build','--offline','--release','--bin','dict_atom_probe','--bin','state_order_probe','--bin','function_state_probe','--bin','corpus_function_state_probe'],cwd=ROOT,check=True)
for prefix,generator,oracle,probe in [('atom','dict_atom_cases.py','dict_atom_oracle.exs','dict_atom_probe'),('order','state_order_cases.py','state_order_oracle.exs','state_order_probe'),('function','function_state_cases.py','function_state_oracle.exs','function_state_probe')]:
 cases=D/(prefix+'_cases.jsonl');native=D/(prefix+'_native.jsonl');rust=D/(prefix+'_rust.jsonl')
 run([sys.executable,generator],target=cases)
 run(['elixir',oracle],cases,native)
 if prefix=='function':
  run([sys.executable,'verify_state.py','--make-inputs']);cases=D/'function_inputs.jsonl'
 run([str(ROOT/'target/release'/probe)],cases,rust)
run(['elixir','corpus_function_state_oracle.exs'],D/'corpus_inputs.jsonl',D/'corpus_function_native.jsonl')
for row in map(json.loads,(D/'corpus_function_native.jsonl').read_text().splitlines()):retain(Path(row['proof_core_json']))
run([str(ROOT/'target/release/corpus_function_state_probe')],D/'corpus_function_native.jsonl',D/'corpus_function_rust.jsonl')
run([str(ROOT/'target/release/corpus_function_state_probe'),'--derive-graph'],D/'corpus_function_native.jsonl',D/'corpus_function_derived.jsonl')
native=[json.loads(s)for s in (D/'corpus_function_native.jsonl').read_text().splitlines()]
derived=[json.loads(s)for s in (D/'corpus_function_derived.jsonl').read_text().splitlines()]
assert len(native)==len(derived)==327 and native==derived,'derived source graph does not reproduce module initialization'
run([sys.executable,'verify_state.py'])
assert before==hashes(),'source changed during verification'
assert all(digest(Path(p))==h for p,h in inputs.items()),'proof inputs changed'
report=json.loads((D/'report.json').read_text());report.update(complete_function_state=True,complete_source_graph_initialization=True,complete_module_inference=False,implementation_sha256=before,inputs_sha256=inputs)
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
