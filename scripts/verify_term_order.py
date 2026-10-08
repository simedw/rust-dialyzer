#!/usr/bin/env python3
import hashlib,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.cache/term_order';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/name for name in ['src/term_order.rs','src/core.rs','src/bin/term_order_probe.rs','scripts/core_export.exs','scripts/term_order_probes/term_order_cases.py','scripts/term_order_probes/term_order_oracle.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def hashes():return {str(path.relative_to(ROOT)):digest(path) for path in files}
implementation=hashes()
subprocess.run(['cargo','build','--release','--offline','--bin','term_order_probe'],cwd=ROOT,check=True)
subprocess.run([sys.executable,str(ROOT/'scripts/term_order_probes/term_order_cases.py'),'--probe',str(ROOT/'target/release/term_order_probe')],cwd=ROOT,check=True)
report=json.loads((OUT/'term_order_results.jsonl.comparison.json').read_text())
if report['outcomes']!=31814 or report['matched']!=31814 or report['failed'] or report['failures']:raise RuntimeError('raw term order parity incomplete')
if implementation!=hashes():raise RuntimeError('raw term comparator changed during verification')
request=OUT/'term_order_commands.jsonl'
report.update(complete_finite_core_term_order=True,implementation_sha256=implementation,inputs_sha256={str(request.relative_to(ROOT)):digest(request)})
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({key:report[key] for key in ['outcomes','matched','complete_finite_core_term_order']}))
