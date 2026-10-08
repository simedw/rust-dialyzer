#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.cache/fingerprint';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/name for name in ['src/fingerprint.rs','src/external_term.rs','src/term_order.rs','src/core.rs','src/program.rs','src/bin/fingerprint_probe.rs','scripts/fingerprint_oracle.exs','scripts/fingerprint_attributes.exs','scripts/core_export.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def hashes():return {str(p.relative_to(ROOT)):digest(p) for p in files}
implementation=hashes();manifest=ROOT/'.cache/engine/manifest.json';source=json.loads(manifest.read_text())
if len(source)!=327 or len({row['id'] for row in source})!=327:raise RuntimeError('Core label corpus denominator changed')
requests=[dict(id=row['id'],core_json=row['core_json'],beams_dir=row['beams_dir']) for index,row in enumerate(source)]
attribute_request=OUT/'attribute-requests.jsonl'
with attribute_request.open('w') as output:subprocess.run(['elixir','scripts/fingerprint_attributes.exs',str(OUT/'attributes')],cwd=ROOT,stdout=output,check=True)
attributes=[json.loads(line) for line in attribute_request.read_text().splitlines()]
if len(attributes)!=32 or len({row['id'] for row in attributes})!=32:raise RuntimeError('map-attribute fingerprint denominator changed')
requests+=attributes
for row in requests:
 row["proof_core_json"]=str(OUT/"native-core"/(row["id"].replace("/","_")+".json"))
(OUT/"native-core").mkdir(parents=True,exist_ok=True)
inputs={str(manifest.relative_to(ROOT)):digest(manifest),**{str(Path(row['core_json']).relative_to(ROOT)):digest(Path(row['core_json'])) for row in requests},**{str(path.relative_to(ROOT)):digest(path) for row in requests for path in Path(row['beams_dir']).glob('*.beam')}}
subprocess.run(['cargo','build','--release','--offline','--bin','fingerprint_probe'],cwd=ROOT,check=True)
request=OUT/'requests.jsonl';request.write_text(''.join(json.dumps(row)+'\n' for row in requests))
with request.open() as inp,(OUT/'native.jsonl').open('w') as out:subprocess.run(['elixir','scripts/fingerprint_oracle.exs'],cwd=ROOT,stdin=inp,stdout=out,check=True)
proof_requests=[dict(row,core_json=row['proof_core_json'])for row in requests]
proof_request=OUT/'proof-requests.jsonl';proof_request.write_text(''.join(json.dumps(row)+'\n'for row in proof_requests))
inputs.update({str(Path(row['core_json']).relative_to(ROOT)):digest(Path(row['core_json']))for row in proof_requests})
with proof_request.open() as inp,(OUT/'rust.jsonl').open('w') as out:subprocess.run([str(ROOT/'target/release/fingerprint_probe')],cwd=ROOT,stdin=inp,stdout=out,check=True)
native=[json.loads(line) for line in (OUT/'native.jsonl').read_text().splitlines()];rust=[json.loads(line) for line in (OUT/'rust.jsonl').read_text().splitlines()]
if len(native)!=359 or len(rust)!=359 or [row['id'] for row in native]!=[row['id'] for row in requests] or [row['id'] for row in rust]!=[row['id'] for row in requests]:raise RuntimeError('Core label outcome identities changed')
failures=[dict(request=row,native=x,rust=y) for row,x,y in zip(requests,native,rust) if x!=y]
if failures:
    (OUT/'failures.json').write_text(json.dumps(failures[:10],indent=2));raise RuntimeError(f'{len(failures)} Core label differences')
if implementation!=hashes() or inputs!={name:digest(ROOT/name) for name in inputs}:raise RuntimeError('Core label evidence changed during verification')
report=dict(commands=359,matched=359,corpus_cases=327,attribute_cases=32,complete_compiler_form_fingerprints=True,complete_snapshot_lifecycle=False,modules=sum(len(row['modules']) for row in native),implementation_sha256=implementation,inputs_sha256=inputs)
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({key:report[key] for key in ['commands','matched','modules']}))
