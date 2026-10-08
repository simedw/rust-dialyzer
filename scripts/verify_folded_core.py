#!/usr/bin/env python3
import hashlib,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/folded_core';OUT.mkdir(parents=True,exist_ok=True);(OUT/'data').mkdir(exist_ok=True)
HELP=ROOT/'scripts/guard_probes'
files=list((ROOT/'src').rglob('*.rs'))+[HELP/n for n in ('folded_core_oracle.exs','corpus_folded_core_oracle.exs')]+[ROOT/'scripts/pattern_probes/binder_oracle.exs',ROOT/'scripts/pattern_probes/verify_binder.py',ROOT/'scripts/type_codec.exs',ROOT/'scripts/type_wire.exs',ROOT/'scripts/core_export.exs',Path(__file__).resolve(),ROOT/'Cargo.toml',ROOT/'Cargo.lock']
beams=subprocess.check_output(['elixir','-e','Enum.each([:cerl,:cerl_prettypr,:prettypr,:dialyzer_utils],fn m->IO.puts(:code.which(m))end)'],cwd=ROOT,text=True).splitlines();files.extend(map(Path,beams))
def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files}
before=hashes();inputs={}
def retain(p):inputs[str(p)]=digest(p)
def run(command,source,target):
 retain(source)
 with source.open('rb')as i,target.open('wb')as o:subprocess.run(command,cwd=OUT,stdin=i,stdout=o,check=True)
manifest=json.loads((ROOT/'.cache/core/report.json').read_text())['cases'];assert len(manifest)==327
for r in manifest:retain(r['core_json'])
synthetic={r['core_json']:r['id']for r in map(json.loads,(ROOT/'.cache/patterns/expanded.jsonl').read_text().splitlines())};assert len(synthetic)==192
for p in synthetic:retain(p)
corpus=OUT/'corpus_inputs.jsonl';corpus.write_text(''.join(json.dumps({'id':r['id'],'core_json':r['core_json']})+'\n'for r in manifest))
cases=OUT/'synthetic_inputs.jsonl';cases.write_text(''.join(json.dumps({'id':i,'core_json':p})+'\n'for p,i in synthetic.items()))
subprocess.run(['cargo','build','--offline','--release','--bin','folded_core_probe','--bin','corpus_folded_core_probe','--bin','shared_warning_binder_probe'],cwd=ROOT,check=True)
counts={}
for prefix,source,oracle,probe,expected,modules in [('synthetic',cases,'folded_core_oracle.exs','folded_core_probe',1272,192),('corpus',corpus,'corpus_folded_core_oracle.exs','corpus_folded_core_probe',75391,571)]:
 native=OUT/(prefix+'.native.jsonl');actual=OUT/(prefix+'.rust.jsonl');run(['elixir',str(HELP/oracle)],source,native)
 for r in map(json.loads,native.read_text().splitlines()):
  if 'core_json'in r:retain(r['core_json'])
 run([str(ROOT/'target/release'/probe)],native,actual)
 rows=[json.loads(s)for s in actual.read_text().splitlines()];ns=[json.loads(s)for s in native.read_text().splitlines()]
 assert len(rows)==len(ns)==(192 if prefix=='synthetic' else 327)
 assert all(a['id']==b['id']and not b['errors']for a,b in zip(ns,rows)),rows
 assert sum(r['cases']for r in rows)==expected
 if prefix=='corpus':assert sum(r['modules']for r in rows)==modules
 counts[prefix]={'cases':len(rows),'outcomes':expected,'modules':modules}
shared_native=OUT/'shared_warning.native.jsonl';run(['elixir',str(ROOT/'scripts/pattern_probes/binder_oracle.exs')],ROOT/'.cache/patterns/expanded.jsonl',shared_native)
shared=OUT/'shared_warning.rust.jsonl';run([str(ROOT/'target/release/shared_warning_binder_probe')],ROOT/'.cache/patterns/expanded.jsonl',shared)
result=json.loads(subprocess.check_output([sys.executable,str(ROOT/'scripts/pattern_probes/verify_binder.py'),str(shared_native),str(shared),'--report',str(OUT/'failures.json')],cwd=ROOT,text=True));assert result['outcomes']==16128 and not result['mismatches'];counts['shared_warning_binder']=result
assert before==hashes();assert all(digest(p)==h for p,h in inputs.items())
report={'gates':counts,'complete_captured_literal_folding':True,'complete_guard_traversal':False,'implementation_sha256':before,'inputs_sha256':inputs};(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(counts))
