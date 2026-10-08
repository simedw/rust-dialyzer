#!/usr/bin/env python3
"""Compare body control and construction against original OTP traversal."""
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/body_flow';OUT.mkdir(parents=True,exist_ok=True)
files=list((ROOT/'src').rglob('*.rs'))+[ROOT/p for p in ('scripts/body_probes/native_body.exs','scripts/dataflow_state_probes/function_state_oracle.exs','scripts/core_export.exs','scripts/type_codec.exs','scripts/type_wire.exs','corpus/bifs.json','Cargo.toml','Cargo.lock')]+[Path(__file__).resolve()]
beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_dataflow,:dialyzer_callgraph,:erl_types,:erl_bif_types,:cerl,:cerl_trees,:dict],fn m->IO.puts(:code.which(m))end)'],cwd=ROOT,text=True).splitlines();files.extend(map(Path,beams))
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files}
before=hashes();subprocess.run(['cargo','build','--offline','--release','--bin','body_probe','--bin','type_probe'],cwd=ROOT,check=True)
binary=ROOT/'target/release/body_probe';inputs={str(binary):digest(binary)};gates={}
for suite,count,shapes in [('structural',29524,122),('maps',11760,30),('binaries',40800,85),('loop',244,122),('cases',80142,111),('case_records',1014,3),('calls_nested',3872,16),('module_calls',34,17),('case_infix',80142,111)]:
 commands=OUT/f'{suite}.commands.jsonl';actual=OUT/f'{suite}.rust.jsonl'
 subprocess.run(['elixir',str(ROOT/'scripts/body_probes/native_body.exs'),str(ROOT),str(commands),suite],cwd=ROOT,check=True)
 inputs[str(commands)]=digest(commands)
 with commands.open('rb')as i,actual.open('wb')as o:subprocess.run([str(binary)],cwd=ROOT,stdin=i,stdout=o,check=True)
 rows=[json.loads(s)for s in commands.read_text().splitlines()];outputs=[json.loads(s)for s in actual.read_text().splitlines()];assert len(rows)==len(outputs)==count
 failures=[{'id':r['id'],'native':r['native'],'rust':s}for r,s in zip(rows,outputs)if r['native']!=s]
 (OUT/f'{suite}.failures.json').write_text(json.dumps(failures[:20],indent=2)+'\n');assert not failures,f'{len(failures)} {suite} differences'
 gates[suite]={'cases':count,'matched':count,'source_shapes':shapes,'warning_cases':sum(bool(r['warnings']['terms'])and r['warnings']['terms'][r['warnings']['root']]['kind']!='nil'for r in rows)}
 print(json.dumps({suite:gates[suite]}),flush=True)
type_probe=ROOT/'target/release/type_probe';inputs[str(type_probe)]=digest(type_probe)
try:
 subprocess.run(['elixir','-e','IO.puts("number_values_unit_started");IO.inspect(:erl_types.t_number_vals(:unit))'],cwd=ROOT,capture_output=True,timeout=5,check=True)
 raise AssertionError('original NumberValues(Unit) unexpectedly terminated')
except subprocess.TimeoutExpired as error:
 assert b'number_values_unit_started' in (error.stdout or b''), 'native helper did not reach its operation before timeout'
request={'op':'getter','name':'number_values','a':{'kind':'unit'}}
unit=json.loads(subprocess.check_output([str(type_probe)],cwd=ROOT,input=(json.dumps(request)+'\n').encode()))
assert unit.get('error','').startswith('native_nonterminating:'),unit
boundary={'native_operation':'erl_types:t_number_vals(unit)','original_native_timeout_seconds':5,'rust':unit,'binary_size_fixture':'Unit excluded from size domains; retained among value domains'}
(OUT/'nontermination.json').write_text(json.dumps(boundary,indent=2)+'\n')
assert before==hashes();assert all(digest(Path(p))==h for p,h in inputs.items())
report={'cases':sum(g['cases']for g in gates.values()),'native_nontermination_boundaries':[boundary],'gates':gates,'complete_captured_structural_body_flow':True,'complete_captured_map_binary_construction':True,'complete_module_inference':False,'phase_fixture':'explicit all-escaping functions with empty raw signatures/contracts; no native inferred facts','checked':['result types','complete type/substitution/modified maps','function entries and captured environments','FIFO queue and reachable functions','exact raw warning terms'],'unsupported':['complete module work loop','malformed segment type exception payload','native t_number_vals(unit) nontermination: Unit is excluded from binary size fixture domains'],'implementation_sha256':before,'inputs_sha256':inputs}
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'cases':report['cases'],'complete_module_inference':False}))
