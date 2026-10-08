#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.cache/constructors';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/name for name in ['src/lattice.rs','src/type_api.rs','src/native_order.rs','src/runtime_term.rs','src/presentation.rs','src/bin/constructor_probe.rs','scripts/constructor_probes/constructor_oracle.exs','scripts/constructor_probes/constructor_cases.jsonl','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def hashes():return {str(p.relative_to(ROOT)):digest(p) for p in files}
implementation=hashes()
subprocess.run(['cargo','build','--release','--offline','--bin','constructor_probe'],cwd=ROOT,check=True)
requests=ROOT/'scripts/constructor_probes/constructor_cases.jsonl';results=OUT/'results.jsonl'
rows=[json.loads(line) for line in requests.read_text().splitlines()]
if len(rows)!=10369:raise RuntimeError('constructor denominator changed')
with requests.open() as source,results.open('w') as output:subprocess.run([str(ROOT/'target/release/constructor_probe')],stdin=source,stdout=output,cwd=ROOT,check=True)
if len(results.read_text().splitlines())!=len(rows):raise RuntimeError('constructor results truncated')
summary=json.loads(subprocess.check_output(['elixir','scripts/constructor_probes/constructor_oracle.exs',str(requests),str(results)],cwd=ROOT))
if summary['cases']!=10369 or summary['failures']:raise RuntimeError('constructor parity incomplete')
if hashes()!=implementation:raise RuntimeError('constructor implementation changed')
summary.update(complete_typed_constructor_boundary=True,complete_arbitrary_constructor_boundary=False,complete_raw_type_algebra=False,implementation_sha256=implementation,scope='Typed integer bitstring parameters and range endpoints; raw negative bitstring records are transport-only')
(OUT/'report.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps({key:summary[key] for key in ['cases','failures','complete_typed_constructor_boundary']}))
