#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.cache/literal_types';OUT.mkdir(parents=True,exist_ok=True)
source=ROOT/'.cache/term_order/term_order_commands.jsonl'
files=[ROOT/name for name in ['src/literal_types.rs','src/term_order.rs','src/core.rs','src/lattice.rs','src/type_api.rs','src/runtime_term.rs','src/native_order.rs','src/presentation.rs','src/presentation_form.rs','src/bin/literal_types_probe.rs','scripts/literal_types_oracle.exs','scripts/core_export.exs','scripts/type_wire.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p.relative_to(ROOT)):digest(p) for p in files}
implementation=hashes();input_hash=digest(source)
# Reuse independently generated finite literal families, excluding ordering-only deep stress rows.
rows=[dict(id=i,arena=json.loads(line)['a']) for i,line in enumerate(source.read_text().splitlines()[:31800])]
if len(rows)!=31800:raise RuntimeError('literal input denominator changed')
request=OUT/'requests.jsonl';request.write_text(''.join(json.dumps(row)+'\n' for row in rows))
subprocess.run(['cargo','build','--release','--offline','--bin','literal_types_probe'],cwd=ROOT,check=True)
for command,name in [(['elixir','scripts/literal_types_oracle.exs'],'native'),([str(ROOT/'target/release/literal_types_probe')],'rust')]:
 with request.open() as inp,(OUT/f'{name}.jsonl').open('w') as out:subprocess.run(command,cwd=ROOT,stdin=inp,stdout=out,check=True)
native=[json.loads(line) for line in (OUT/'native.jsonl').read_text().splitlines()];rust=[json.loads(line) for line in (OUT/'rust.jsonl').read_text().splitlines()]
if len(native)!=len(rows) or len(rust)!=len(rows):raise RuntimeError('literal outcome denominator changed')
failures=[dict(request=row,native=x,rust=y) for row,x,y in zip(rows,native,rust) if x!=y]
if failures:
 (OUT/'failures.json').write_text(json.dumps(failures[:20],indent=2));raise RuntimeError(f'{len(failures)} literal type differences')
if implementation!=hashes() or input_hash!=digest(source):raise RuntimeError('literal evidence changed during verification')
report=dict(commands=len(rows),matched=len(rows),complete_finite_literal_adapter=True,complete_runtime_term_boundary=False,scope='Finite Core literal families; deep type construction and runtime identity families remain separate boundaries',implementation_sha256=implementation,inputs_sha256={str(source.relative_to(ROOT)):input_hash})
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k] for k in ['commands','matched']}))
