#!/usr/bin/env python3
import hashlib,json,subprocess,collections,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'.cache/warning_render';OUT.mkdir(parents=True,exist_ok=True)
HELPERS=ROOT/'scripts/warning_render_probes'
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
manifest=ROOT/'.cache/engine/manifest.json';cases=json.loads(manifest.read_text())
if len(cases)!=327 or len({row['id'] for row in cases})!=327:raise RuntimeError('warning render corpus changed')
inputs={str(path.relative_to(ROOT)):digest(path) for path in [manifest,*[Path(row['native_json']) for row in cases]]}
subprocess.run(['elixir',str(HELPERS/'inventory.exs'),str(ROOT)],cwd=ROOT,check=True)
subprocess.run([sys.executable,str(HELPERS/'generate.py')],cwd=ROOT,check=True)
files=[ROOT/name for name in ['src/warning_render.rs','src/render_source.rs','src/core.rs','src/bifs/boundary.rs','src/lattice.rs','src/type_api.rs','src/native_order.rs','src/runtime_term.rs','src/presentation.rs','src/presentation_form.rs','src/bin/render_probe.rs','src/bin/ordering_probe.rs','src/bin/layout_probe.rs','src/bin/doc_probe.rs','src/term_order.rs','scripts/core_export.exs','Cargo.toml','Cargo.lock']]+list((ROOT/'src/erlang_syntax').glob('*.rs'))+list(HELPERS.iterdir())+[Path(__file__).resolve()]
def hashes():return {str(path.relative_to(ROOT)):digest(path) for path in files if path.is_file()}
implementation=hashes()
subprocess.run(['cargo','build','--release','--offline','--bin','render_probe','--bin','ordering_probe','--bin','layout_probe','--bin','doc_probe'],cwd=ROOT,check=True)
with (OUT/'synthetic.native.jsonl').open('w') as out:subprocess.run(['elixir',str(HELPERS/'synthetic.exs')],cwd=ROOT,stdout=out,check=True)
with (OUT/'ordering.native.jsonl').open('w') as out:subprocess.run(['elixir',str(HELPERS/'ordering_oracle.exs'),str(ROOT)],cwd=ROOT,stdout=out,check=True)
summary={}
for name,count,unindented in [('render',15592,7796),('synthetic',1540,1120)]:
    with (OUT/f'{name}.native.jsonl').open() as inp,(OUT/f'{name}.results.jsonl').open('w') as out:subprocess.run([str(ROOT/'target/release/render_probe')],stdin=inp,stdout=out,check=True)
    refs=[json.loads(line) for line in (OUT/f'{name}.native.jsonl').read_text().splitlines()];results=[json.loads(line) for line in (OUT/f'{name}.results.jsonl').read_text().splitlines()]
    if len(refs)!=count or len(results)!=count or [r['id'] for r in refs]!=[r['id'] for r in results]:raise RuntimeError(f'{name} warning outcome identities changed')
    counts=collections.Counter(('indented' if ref['options']['indent'] else 'unindented',r['status'],r['matched']) for ref,r in zip(refs,results))
    failures=sum(not r['matched'] for r in results)
    summary[name]=dict(requests=count,unindented_matched=counts['unindented','rendered',True],indented_matched=counts['indented','rendered',True],indented_explicit_unsupported=counts['indented','unsupported',False],failures=failures)
    if failures or summary[name]['unindented_matched']!=unindented or summary[name]['indented_matched']!=count-unindented:raise RuntimeError(f'{name} unindented parity incomplete')
with (OUT/'ordering.native.jsonl').open() as inp,(OUT/'ordering.results.jsonl').open('w') as out:subprocess.run([str(ROOT/'target/release/ordering_probe')],stdin=inp,stdout=out,check=True)
results=[json.loads(line) for line in (OUT/'ordering.results.jsonl').read_text().splitlines()]
if len(results)!=623 or [r['id'] for r in results]!=list(range(623)) or not all(r['matched'] for r in results):raise RuntimeError('native warning ordering incomplete')
summary['ordering']=dict(requests=623,matched=623)
for name,count,command,probe in [
    ('layout',984,['elixir',str(HELPERS/'layout_inventory.exs'),str(OUT/'render.native.jsonl'),str(OUT/'synthetic.native.jsonl')],'layout_probe'),
    ('doc',1093,['elixir',str(HELPERS/'doc_oracle.exs'),str(OUT/'layout.native.jsonl')],'doc_probe'),
    ('syntax_cases',363,['elixir',str(HELPERS/'syntax_cases.exs')],'layout_probe')]:
    with (OUT/f'{name}.native.jsonl').open('w') as out:subprocess.run(command,cwd=ROOT,stdout=out,check=True)
    with (OUT/f'{name}.native.jsonl').open() as inp,(OUT/f'{name}.results.jsonl').open('w') as out:subprocess.run([str(ROOT/f'target/release/{probe}')],cwd=ROOT,stdin=inp,stdout=out,check=True)
    refs=[json.loads(line) for line in (OUT/f'{name}.native.jsonl').read_text().splitlines()];results=[json.loads(line) for line in (OUT/f'{name}.results.jsonl').read_text().splitlines()]
    if len(refs)!=count or len(results)!=count or [r['id'] for r in refs]!=[r['id'] for r in results] or not all(r['matched'] for r in results):raise RuntimeError(f'{name} complete parity not achieved')
    summary[name]=dict(requests=count,matched=count)
inventory=json.loads((OUT/'render.inventory.json').read_text())
if inventory['cases']!=327 or inventory['warnings']!=1949 or inventory['source_clauses']!=56:raise RuntimeError('warning source/corpus inventory changed')
if implementation!=hashes() or inputs!={path:digest(ROOT/path) for path in inputs}:raise RuntimeError('warning rendering evidence changed during verification')
summary.update(complete_unindented_render=True,complete_default_render=True,complete_raw_term_ordering=True,complete_warning_production=False,complete_arbitrary_warning_boundary=False,scope="All captured and source-derived messages/options; finite raw ordering; explicit rendering resource limits and general syntax gaps remain",inventory=inventory,implementation_sha256=implementation,inputs_sha256=inputs)
(OUT/'report.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({key:summary[key] for key in ['render','synthetic','ordering','complete_default_render']}))
