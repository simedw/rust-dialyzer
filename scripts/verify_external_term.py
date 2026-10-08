#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/external_term';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/name for name in ['src/external_term.rs','src/core.rs','src/term_order.rs','src/type_exports.rs','src/bin/external_term_probe.rs','scripts/external_term_oracle.exs','scripts/core_export.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p.relative_to(ROOT)):digest(p) for p in files}
implementation=hashes();source=ROOT/'.cache/term_order/term_order_commands.jsonl';input_hash=digest(source)
rows=[dict(id=i,arena=json.loads(line)['a']) for i,line in enumerate(source.read_text().splitlines()[:31800]) if i%6==0]
if len(rows)!=5300:raise RuntimeError('ETF seed denominator changed')
seed=OUT/'seed.jsonl';seed.write_text(''.join(json.dumps(row)+'\n' for row in rows))
request=OUT/'requests.jsonl'
with request.open('w') as out:subprocess.run(['elixir','scripts/external_term_oracle.exs','generate',str(seed)],cwd=ROOT,stdout=out,check=True)
# Native malformed byte acceptance is checked separately from valid value roundtrips.
malformed=[[],[131],[0],[131,255],[131,97],[131,116,0,0,0,2,97,1,97,2,97,1,97,3],[131,70]+[255]*8,[131,77,0,0,0,1,0,0],[131,80,0,0,0,1,0],[131,110,1,2,1],[131,97,1,255]]
with request.open('a') as out:
 for i,bytes_ in enumerate(malformed):out.write(json.dumps(dict(id=f'malformed/{i}',bytes=bytes_))+'\n')
subprocess.run(['cargo','build','--release','--offline','--bin','external_term_probe'],cwd=ROOT,check=True)
result=OUT/'results.jsonl'
with request.open() as inp,result.open('w') as out:subprocess.run([str(ROOT/'target/release/external_term_probe')],cwd=ROOT,stdin=inp,stdout=out,check=True)
summary=json.loads(subprocess.check_output(['elixir','scripts/external_term_oracle.exs','compare',str(request),str(result)],cwd=ROOT))
if summary['commands']!=15911 or summary['matched']!=15911 or summary['failures']:raise RuntimeError('ETF full gate failed')
if implementation!=hashes() or input_hash!=digest(source):raise RuntimeError('ETF proof changed during verification')
summary.update(complete_finite_etf_transport=True,complete_runtime_identity_transport=False,complete_snapshot_lifecycle=False,implementation_sha256=implementation,inputs_sha256={str(source.relative_to(ROOT)):input_hash})
(OUT/'report.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps({k:summary[k] for k in ['commands','matched']}))
