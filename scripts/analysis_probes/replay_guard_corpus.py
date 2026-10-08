#!/usr/bin/env python3
"""Replay frozen original compiler guards through the integrated Rust build."""
import argparse,hashlib,json,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--checkpoint',type=Path,required=True);a=p.parse_args();ROOT=Path(__file__).resolve().parents[2];proof=a.checkpoint.resolve()/'proof';OUT=ROOT/'.cache/guard_corpus';OUT.mkdir(parents=True,exist_ok=True)
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
manifest=json.loads((proof/'guard_corpus_proof_inputs.json').read_text())
for filename,digest in manifest.items():assert sha(Path(filename))==digest,filename
native=proof/'guard_corpus_complete_native.jsonl';native_sha=sha(native);binary=ROOT/'target/release/guard_corpus_probe';binary_sha=sha(binary)
files=list((ROOT/'src').rglob('*.rs'))+[ROOT/p for p in ['Cargo.toml','Cargo.lock','corpus/bifs.json','corpus/dependency_policy.json']]+[Path(__file__).resolve()];sources={str(f):sha(f) for f in files};start=time.monotonic();actual=OUT/'rust.jsonl'
commands=OUT/'commands.jsonl'
with native.open() as source,commands.open('w') as dest:
 for line in source:
  row=json.loads(line)
  if not Path(row['core']).is_absolute():
   matches=[filename for filename in manifest if filename.endswith('/'+row['core'])]
   assert len(matches)==1,(row['core'],matches)
   row['core']=matches[0]
  dest.write(json.dumps(row)+'\n')
command_sha=sha(commands)
with commands.open('rb') as i,actual.open('wb') as o:subprocess.run([str(binary),str(ROOT/'corpus/bifs.json')],stdin=i,stdout=o,cwd=ROOT,check=True)
rows=[json.loads(line) for line in actual.read_text().splitlines()];assert len(rows)==327
assert all(row['matches']==row['cases'] and row['unsupported']==0 and row['mismatches']==[] for row in rows)
assert sum(row['roots'] for row in rows)==59189 and sum(row['cases'] for row in rows)==355134
assert sources=={str(f):sha(f) for f in files};assert sha(native)==native_sha and sha(binary)==binary_sha and sha(commands)==command_sha
for filename,digest in manifest.items():assert sha(Path(filename))==digest,filename
report=dict(groups=len(rows),roots=59189,outcomes=355134,matches=355134,original_proof_manifest_sha256=sha(proof/'guard_corpus_proof_inputs.json'),native_sha256=native_sha,transport_commands_sha256=command_sha,binary_sha256=binary_sha,source_sha256=sources,output_sha256=sha(actual),elapsed_seconds=time.monotonic()-start,whole_module_inference=False,scope='Replay of frozen original same-VM guard outcomes and filename proof with integrated Rust metadata preparation, maps and raw warnings.')
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print({k:report[k] for k in ['groups','roots','outcomes','matches','elapsed_seconds','whole_module_inference']})
