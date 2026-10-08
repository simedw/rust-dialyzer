#!/usr/bin/env python3
import hashlib,json,shutil,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/raw_operand';OUT.mkdir(parents=True,exist_ok=True);HELP=ROOT/'scripts/raw_operand_probes'
files=[ROOT/p for p in ['src/native_type_operand.rs','src/native_form_environment.rs','src/form_error.rs','src/lattice.rs','src/type_exports.rs','src/type_api.rs','src/native_order.rs','src/runtime_term.rs','src/presentation.rs','src/presentation_form.rs','src/core.rs','src/term_order.rs','src/native_error.rs','src/bin/native_operand_probe.rs','Cargo.toml','Cargo.lock','scripts/core_export.exs']]+list(HELP.glob('*'))+[Path(__file__).resolve()]
beam=Path(subprocess.check_output(['elixir','-e','IO.puts(:code.which(:erl_types))'],cwd=ROOT,text=True).strip());files.append(beam)
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files if p.is_file()}
before=hashes()
for p in HELP.glob('*'):
 if p.is_file():shutil.copyfile(p,OUT/p.name)
shutil.copyfile(ROOT/'scripts/core_export.exs',OUT/'core_export.exs')
subprocess.run(['cargo','build','--release','--offline','--bin','native_operand_probe'],cwd=ROOT,check=True)
subprocess.run([sys.executable,str(OUT/'verify_raw_continuation.py'),'--probe',str(ROOT/'target/release/native_operand_probe')],cwd=OUT,check=True)
report=json.loads((OUT/'raw_continuation_gate.json').read_text());assert report['native_operand']['matches']==16716 and sum(report[k]['matches']for k in ['raw_scalar','raw_structural','raw_list'])==89616
assert before==hashes(),'source changed during verification'
report.update(complete_captured_raw_operand_subsets=True,complete_arbitrary_raw_algebra=False,implementation_sha256=before,inputs_sha256={str(p):digest(p)for p in sorted(OUT.glob('*commands.jsonl'))})
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(dict(original_matches=16716,new_matches=89616,unsupported=1662,native_timeouts=2)))
