#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.cache/dataflow_nodes';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/name for name in ['src/labels.rs','src/core.rs','src/program.rs','src/bin/dataflow_nodes_probe.rs','src/dataflow_nodes.rs','src/dataflow_map.rs','src/literal_types.rs','src/term_order.rs','src/lattice.rs','src/type_api.rs','src/native_order.rs','src/runtime_term.rs','src/presentation.rs','src/presentation_form.rs','scripts/dataflow_nodes_oracle.exs','scripts/dataflow_map_oracle.exs','scripts/type_codec.exs','scripts/type_wire.exs','scripts/core_export.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def hashes():return {str(p.relative_to(ROOT)):digest(p) for p in files}
implementation=hashes();manifest=ROOT/'.cache/engine/manifest.json';source=json.loads(manifest.read_text())
if len(source)!=327 or len({row['id'] for row in source})!=327:raise RuntimeError('Core label corpus denominator changed')
requests=[dict(id=row['id'],core_json=row['core_json'],initial=(index%17)*137) for index,row in enumerate(source)]
inputs={str(manifest.relative_to(ROOT)):digest(manifest),**{str(Path(row['core_json']).relative_to(ROOT)):digest(Path(row['core_json'])) for row in requests}}
subprocess.run(['cargo','build','--release','--offline','--bin','dataflow_nodes_probe'],cwd=ROOT,check=True)
request=OUT/'requests.jsonl';request.write_text(''.join(json.dumps(row)+'\n' for row in requests))
for command,name in [(['elixir','scripts/dataflow_nodes_oracle.exs','scripts/dataflow_map_oracle.exs','scripts/type_codec.exs','scripts/type_wire.exs'],'native'),([str(ROOT/'target/release/dataflow_nodes_probe')],'rust')]:
    with request.open() as inp,(OUT/f'{name}.jsonl').open('w') as out:subprocess.run(command,cwd=ROOT,stdin=inp,stdout=out,check=True)
native=[json.loads(line) for line in (OUT/'native.jsonl').read_text().splitlines()];rust=[json.loads(line) for line in (OUT/'rust.jsonl').read_text().splitlines()]
if len(native)!=327 or len(rust)!=327 or [row['id'] for row in native]!=[row['id'] for row in requests] or [row['id'] for row in rust]!=[row['id'] for row in requests]:raise RuntimeError('Core label outcome identities changed')
failures=[dict(request=row,native=x,rust=y) for row,x,y in zip(requests,native,rust) if x!=y]
if failures:
    (OUT/'failures.json').write_text(json.dumps(failures[:10],indent=2));raise RuntimeError(f'{len(failures)} Core label differences')
if implementation!=hashes() or inputs!={name:digest(ROOT/name) for name in inputs}:raise RuntimeError('Core label evidence changed during verification')
report=dict(commands=327,matched=327,complete_native_fresh_traversal=True,complete_core_node_adapter=False,complete_pattern_binding=False,variable_entries=sum(len(module['types']) for row in native for module in row['modules']),implementation_sha256=implementation,inputs_sha256=inputs)
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({key:report[key] for key in ['commands','matched','variable_entries']}))
