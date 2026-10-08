#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
R=Path(__file__).resolve().parents[2];C=Path('/private/tmp/rust-dialyzer-diagnostic-adapter-checkpoint-1');O=R/'.cache/diagnostic_adapter';O.mkdir(parents=True,exist_ok=True)
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
j=json.loads((C/'checkpoint.json').read_text())
for n,h in j['files'].items():assert sha(C/n)==h,n
native=json.loads((C/'verified-commands.jsonl.source.json').read_text())
for f in native['files']:assert sha(f['path'])==f['sha256'],f['path']
files=list((R/'src').rglob('*.rs'))+[R/'Cargo.toml',R/'Cargo.lock',Path(__file__).resolve(),C/'verified-commands.jsonl',C/'library-v3.json',R/'target/release/diagnostic_adapter_probe']
before={str(p):sha(p) for p in files}
with (C/'verified-commands.jsonl').open('rb') as i,(O/'rust.jsonl').open('wb') as o:subprocess.run([str(R/'target/release/diagnostic_adapter_probe'),str(C/'library-v3.json')],stdin=i,stdout=o,check=True,cwd=R)
rows=[json.loads(x) for x in (O/'rust.jsonl').read_text().splitlines()];assert len(rows)==144
assert all(r.get('matched') and r.get('callback_order_matched') and r.get('event_boundaries') and r.get('schema_boundaries') for r in rows),[r for r in rows if not r.get('matched')][:3]
assert before=={str(p):sha(p) for p in files},'proof source changed'
counts={k:sum(r[k] for r in rows) for k in ['contract_warnings','callback_warnings','missing_warnings']}
assert counts==dict(contract_warnings=1638,callback_warnings=13405,missing_warnings=24)
(O/'report.json').write_text(json.dumps(dict(cases=144,matches=144,warnings=counts,source_sha256=before,native_source=native,output_sha256=sha(O/'rust.jsonl'),full_inference=False),indent=2)+'\n');print(counts)
