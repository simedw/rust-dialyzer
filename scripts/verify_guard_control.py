#!/usr/bin/env python3
"""Regenerate native guard control, BIF calls and type-order proofs."""
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/guard_control';OUT.mkdir(parents=True,exist_ok=True)
files=list((ROOT/'src').rglob('*.rs'))+[ROOT/p for p in ['Cargo.toml','Cargo.lock','corpus/bifs.json','scripts/guard_probes/guard_control_oracle.exs','scripts/guard_probes/guard_calls_oracle.exs','scripts/guard_probes/rank_oracle.exs','scripts/guard_probes/rank_cases.jsonl','scripts/pattern_probes/binder_oracle.exs','scripts/core_export.exs','scripts/type_codec.exs','scripts/type_wire.exs']]+[Path(__file__).resolve()]
beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_dataflow,:erl_bif_types,:erl_types,:cerl,:cerl_trees],fn m->IO.puts(:code.which(m))end)'],cwd=ROOT,text=True).splitlines();files.extend(map(Path,beams))
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):sha(p)for p in files}
before=hashes();subprocess.run(['cargo','build','--offline','--release','--bin','guard_control_probe','--bin','rank_probe'],cwd=ROOT,check=True)
binaries=[ROOT/'target/release/guard_control_probe',ROOT/'target/release/rank_probe'];inputs={str(p):sha(p)for p in binaries};gates={}
for suite,script,count,binary in [('control','guard_control_oracle.exs',34800,binaries[0]),('calls','guard_calls_oracle.exs',19980,binaries[0]),('ranks','rank_oracle.exs',3160,binaries[1])]:
 native=OUT/f'{suite}.native.jsonl';actual=OUT/f'{suite}.rust.jsonl'
 with native.open('wb')as o:
  if suite=='ranks':
   with (ROOT/'scripts/guard_probes/rank_cases.jsonl').open('rb')as i:subprocess.run(['elixir',str(ROOT/'scripts/guard_probes'/script)],cwd=ROOT,stdin=i,stdout=o,check=True)
  else:subprocess.run(['elixir',str(ROOT/'scripts/guard_probes'/script)],cwd=ROOT,stdout=o,check=True)
 rows=[json.loads(line)for line in native.read_text().splitlines()];assert len(rows)==count
 inputs[str(native)]=sha(native)
 for row in rows:
  if 'core_json'in row:inputs[row['core_json']]=sha(Path(row['core_json']))
 with native.open('rb')as i,actual.open('wb')as o:subprocess.run([str(binary)],cwd=ROOT,stdin=i,stdout=o,check=True)
 results=[json.loads(line)for line in actual.read_text().splitlines()];assert len(results)==count
 failures=[r for n,r in zip(rows,results)if n['id']!=r['id']or not r['matches']or ('warning_terms'in n.get('ok',{})and not r.get('warning_exact'))]
 (OUT/f'{suite}.failures.json').write_text(json.dumps(failures[:20],indent=2)+'\n');assert not failures,f'{len(failures)} {suite} differences'
 objects=[n.get('ok',{}) for n in rows if isinstance(n.get('ok',{}),dict)]
 gates[suite]={'cases':count,'native_errors':sum('error'in n for n in rows),'failures':sum('failure'in n for n in rows),'returned_warnings':sum(len(n.get('warnings',[]))for n in objects),'raw_warning_logs':sum('warning_terms'in n for n in objects)}
 print(json.dumps({suite:gates[suite]}),flush=True)
assert before==hashes();assert all(sha(Path(p))==h for p,h in inputs.items())
report={'cases':54780,'rank_cases':3160,'gates':gates,'complete_captured_guard_control':True,'complete_guard_interpreter':False,'complete_module_inference':False,'unsupported':['nonliteral equality','boolean composition','function/2 and record/3 guards','case','malformed Core assertions'],'implementation_sha256':before,'inputs_sha256':inputs}
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'cases':54780,'rank_cases':3160,'complete_guard_interpreter':False}))
