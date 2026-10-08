#!/usr/bin/env python3
import hashlib,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/form_exports'
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
files=[*sorted((ROOT/'src').glob('*.rs')),*sorted((ROOT/'src/bifs').rglob('*.rs')),ROOT/'src/bin/cache_probe.rs',ROOT/'src/bin/metadata_exports_probe.rs',*sorted((ROOT/'scripts/form_export_probes').glob('*')),ROOT/'scripts/core_export.exs',ROOT/'Cargo.toml',ROOT/'Cargo.lock',Path(__file__).resolve()]
def hashes():return {str(p.relative_to(ROOT)):digest(p) for p in files if p.is_file()}
implementation=hashes();pool=Path('/private/tmp/rust-dialyzer-lattice/form_presentation_commands.jsonl');pool_hash=digest(pool)
subprocess.run(['cargo','build','--release','--offline','--bin','cache_probe','--bin','metadata_exports_probe'],cwd=ROOT,check=True)
for name,probe in [('cache_cases','cache_probe'),('context_status_cases','cache_probe'),('probe','metadata_exports_probe')]:subprocess.run([sys.executable,str(ROOT/f'scripts/form_export_probes/{name}.py'),'--probe',str(ROOT/f'target/release/{probe}')],cwd=ROOT,check=True)
cache=json.loads((OUT/'cache_results.jsonl.comparison.json').read_text());metadata=json.loads((OUT/'rust.jsonl.comparison.json').read_text())
status=json.loads((OUT/'context_status_results.jsonl.comparison.json').read_text())
if status['outcomes']!=60 or status['matched']!=60 or status['failed']:raise RuntimeError('context membership acceptance changed')
if cache['cases']!=500 or cache['outcomes']!=11000 or cache['matched']!=11000 or cache['failed'] or metadata['count']!=48803 or metadata['diff_count']:raise RuntimeError('form/cache acceptance denominator or result changed')
if implementation!=hashes() or pool_hash!=digest(pool):raise RuntimeError('form/cache evidence changed during verification')
report=dict(complete_form_cache_foundation=True,complete_exported_form_boundary=False,commands=59863,matched=59863,cache=cache,metadata=metadata,context_status=status,unsupported=['exact resolver exception payloads','all remote policy contexts','raw Site filename parity','large recursive form corpus'],implementation_sha256=implementation,inputs_sha256={str(pool):pool_hash})
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k] for k in ['commands','matched','complete_exported_form_boundary']}))
