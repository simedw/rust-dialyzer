#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.cache/snapshot_file';OUT.mkdir(parents=True,exist_ok=True)
files=[ROOT/name for name in ['src/snapshot_file.rs','src/external_term.rs','src/term_order.rs','src/core.rs','src/type_exports.rs','src/bin/snapshot_file_probe.rs','scripts/snapshot_file_oracle.exs','scripts/core_export.exs','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def hashes():return {str(p.relative_to(ROOT)):digest(p) for p in files}
implementation=hashes();base=ROOT/'.cache/corpus/otp/base.plt';base_hash=digest(base)
subprocess.run(['cargo','build','--release','--offline','--bin','snapshot_file_probe'],cwd=ROOT,check=True)
requests=OUT/'requests.jsonl'
with requests.open('w') as output:subprocess.run(['elixir','scripts/snapshot_file_oracle.exs','generate',str(OUT),str(base)],cwd=ROOT,stdout=output,check=True)
rows=[json.loads(line) for line in requests.read_text().splitlines()]
if len(rows)!=824 or len({row['id'] for row in rows})!=824:raise RuntimeError('snapshot file denominator changed')
inputs={row['path']:digest(Path(row['path'])) for row in rows if Path(row['path']).is_file()}
results=OUT/'results.jsonl'
with requests.open() as source,results.open('w') as output:subprocess.run([str(ROOT/'target/release/snapshot_file_probe')],stdin=source,stdout=output,cwd=ROOT,check=True)
summary=json.loads(subprocess.check_output(['elixir','scripts/snapshot_file_oracle.exs','compare',str(requests),str(results)],cwd=ROOT))
if summary['commands']!=824 or summary['matched']!=824 or summary['failures'] or summary['native_table_reloads']!=2:raise RuntimeError('snapshot file acceptance changed')
if hashes()!=implementation or digest(base)!=base_hash or inputs!={name:digest(Path(name)) for name in inputs}:raise RuntimeError('snapshot input or implementation changed')
summary.update(complete_finite_snapshot_file_transport=True,complete_snapshot_lifecycle=False,complete_arbitrary_snapshot_boundary=False,implementation_sha256=implementation,inputs_sha256={str(base.relative_to(ROOT)):base_hash},scope='Finite ETF root records, lazy inner fields, legacy incremental data, actual classic/IPLT native table reload; runtime identities and production analysis lifecycle remain open')
(OUT/'report.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps({key:summary[key] for key in ['commands','matched','native_table_reloads']}))
