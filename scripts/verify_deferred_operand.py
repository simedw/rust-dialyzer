#!/usr/bin/env python3
import hashlib,json,shutil,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/deferred_operand';OUT.mkdir(parents=True,exist_ok=True)
HELP=ROOT/'scripts/deferred_probes'
files=[*sorted((ROOT/'src').rglob('*.rs')),*sorted(HELP.glob('*')),ROOT/'scripts/core_export.exs',ROOT/'Cargo.toml',ROOT/'Cargo.lock',Path(__file__).resolve()]
beams=subprocess.check_output(['elixir','-e','Enum.each([:erl_types,:dialyzer_utils],fn m -> IO.puts(:code.which(m)) end)'],cwd=ROOT,text=True).splitlines();files.extend(map(Path,beams))
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files if p.is_file()}
before=hashes()
for p in HELP.glob('*'):
 if p.is_file():shutil.copyfile(p,OUT/p.name)
shutil.copyfile(ROOT/'scripts/core_export.exs',OUT/'core_export.exs')
subprocess.run(['cargo','build','--release','--offline','--bin','cache_probe','--bin','native_operand_probe'],cwd=ROOT,check=True)
subprocess.run([sys.executable,str(OUT/'verify_deferred_operand.py'),'--cache-probe',str(ROOT/'target/release/cache_probe'),'--operand-probe',str(ROOT/'target/release/native_operand_probe')],cwd=OUT,check=True)
report=json.loads((OUT/'deferred_operand_gate.json').read_text());assert report['deferred_context_matches']==572 and report['native_operand']['matches']==16716 and report['native_operand']['unsupported']==1662 and len(report['native_operand']['oracle_timeouts'])==2
assert before==hashes(),'source changed during verification'
report.update(complete_captured_deferred_context=True,complete_arbitrary_raw_algebra=False,implementation_sha256=before,inputs_sha256={str(p):digest(p)for p in sorted(OUT.glob('*.jsonl'))})
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(dict(deferred_matches=572,raw_matches=16716,raw_unsupported=1662,raw_native_timeouts=2)))
