#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/guard_producer';OUT.mkdir(parents=True,exist_ok=True)
HELP=ROOT/'scripts/guard_probes'
files=list((ROOT/'src').rglob('*.rs'))+[HELP/n for n in ('guard_producer_oracle.exs','guard_producer_context_inputs.jsonl')]+[ROOT/p for p in ('scripts/pattern_probes/binder_oracle.exs','scripts/core_export.exs','scripts/type_codec.exs','scripts/type_wire.exs','Cargo.toml','Cargo.lock')]+[Path(__file__).resolve()]
beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_dataflow,:dialyzer_utils,:erl_types,:cerl,:cerl_prettypr,:prettypr],fn m->IO.puts(:code.which(m))end)'],cwd=ROOT,text=True).splitlines();files.extend(map(Path,beams))
def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files}
before=hashes();inputs={}
def retain(p):inputs[str(p)]=digest(p)
def run(command,source,target):
 retain(source)
 with source.open('rb')as i,target.open('wb')as o:subprocess.run(command,cwd=OUT,stdin=i,stdout=o,check=True)
cores={r['core_json']:r['id']for r in map(json.loads,(ROOT/'.cache/patterns/expanded.jsonl').read_text().splitlines())};assert len(cores)==192
for p in cores:retain(p)
cases=OUT/'synthetic.jsonl';cases.write_text(''.join(json.dumps({'id':i,'core':json.loads(Path(p).read_text())})+'\n'for p,i in cores.items()))
subprocess.run(['cargo','build','--offline','--release','--bin','guard_producer_probe'],cwd=ROOT,check=True)
counts={};errors=0
for prefix,source,count,expected in [('synthetic',cases,192,71520),('context',HELP/'guard_producer_context_inputs.jsonl',1,220)]:
 native=OUT/(prefix+'.native.jsonl');actual=OUT/(prefix+'.rust.jsonl');run(['elixir',str(HELP/'guard_producer_oracle.exs')],source,native);run([str(ROOT/'target/release/guard_producer_probe')],native,actual)
 ns=[json.loads(s)for s in native.read_text().splitlines()];rs=[json.loads(s)for s in actual.read_text().splitlines()]
 assert len(ns)==len(rs)==count and sum(r['cases']for r in rs)==expected
 assert all(a['id']==b['id']and not b['errors']for a,b in zip(ns,rs)),[r for r in rs if r['errors']][:1]
 errors+=sum('error'in outcome['result']for n in ns for module in n['modules']for outcome in module['outcomes']);counts[prefix]={'cases':count,'outcomes':expected}
assert errors==5070
assert before==hashes();assert all(digest(p)==h for p,h in inputs.items())
report={'gates':counts,'outcomes':71740,'native_errors':errors,'complete_captured_guard_argument_producer':True,'complete_guard_traversal':False,'implementation_sha256':before,'inputs_sha256':inputs};(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(counts))
