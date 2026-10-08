#!/usr/bin/env python3
import hashlib,json,os,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/captures';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/p for p in ['src/term_order.rs','src/constraints/closure_order.rs','src/bin/constraints_probe.rs','src/core.rs','Cargo.toml','Cargo.lock','scripts/constraint_probes/native_captures.exs','scripts/core_export.exs']]+[Path(__file__).resolve()]
def hashes():return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest()for p in files}
before=hashes()
subprocess.run(['cargo','build','--release','--offline','--bin','constraints_probe'],cwd=ROOT,check=True)
request=OUT/'commands.jsonl'
subprocess.run(['elixir',str(ROOT/'scripts/constraint_probes/native_captures.exs'),str(request)],cwd=ROOT,check=True)
rows=[json.loads(x)for x in request.read_text().splitlines()];assert len(rows)==26462
result=subprocess.check_output([str(ROOT/'target/release/constraints_probe')],input=request.read_bytes())
(OUT/'results.jsonl').write_bytes(result);results=[json.loads(x)for x in result.splitlines()]
assert len(results)==len(rows)
bad=[{'index':i,'native':r['native_order'],'rust':s}for i,(r,s)in enumerate(zip(rows,results))if s!={'order':r['native_order']}]
if bad:raise RuntimeError(json.dumps(bad[:10]))
assert before==hashes(),'implementation changed during verification'
report=dict(outcomes=len(rows),matched=len(rows),complete_captured_runtime_order=True,complete_arbitrary_runtime_order=False,implementation_sha256=before,inputs_sha256={str(request.relative_to(ROOT)):hashlib.sha256(request.read_bytes()).hexdigest()},unsupported=['comparison between distinct opaque runtime identities'])
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
