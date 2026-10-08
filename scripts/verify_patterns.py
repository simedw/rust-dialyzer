#!/usr/bin/env python3
import hashlib,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/patterns';OUT.mkdir(parents=True,exist_ok=True);(OUT/'data/cores').mkdir(parents=True,exist_ok=True)
HELP=ROOT/'scripts/pattern_probes'
files=[*sorted((ROOT/'src').rglob('*.rs')),*sorted(HELP.glob('*')),ROOT/'scripts/core_export.exs',ROOT/'scripts/type_codec.exs',ROOT/'scripts/type_wire.exs',ROOT/'Cargo.toml',ROOT/'Cargo.lock',Path(__file__).resolve()]
beam_paths=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_dataflow,:dialyzer_utils,:erl_types,:cerl,:cerl_trees,:cerl_prettypr,:prettypr],fn m -> IO.puts(:code.which(m)) end)'],cwd=ROOT,text=True).splitlines();files.extend(map(Path,beam_paths))
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files}
before=hashes();subprocess.run(['cargo','build','--release','--offline','--bin','binder_probe','--bin','corpus_binder_probe','--bin','printer_probe','--bin','corpus_printer_probe'],cwd=ROOT,check=True)
def run(args,dest,source=None):
 with open(OUT/dest,'wb')as out:
  if source:
   with open(OUT/source,'rb')as inp:subprocess.run(args,cwd=OUT,stdin=inp,stdout=out,check=True)
  else:subprocess.run(args,cwd=OUT,stdout=out,check=True)
 return OUT/dest
def native(name,dest,source=None):return run(['elixir',str(HELP/name)],dest,source)
proof_inputs={}
def retain(path):
 path=Path(path);actual=digest(path)
 if str(path) in proof_inputs:assert proof_inputs[str(path)]==actual,'proof input changed before reuse'
 proof_inputs[str(path)]=actual
def rust(name,dest,source):
 retain(OUT/source)
 for line in (OUT/source).read_text().splitlines():
  row=json.loads(line)
  if 'core_json' in row:retain(row['core_json'])
 return run([str(ROOT/'target/release'/name)],dest,source)
def comparison(script,native_name,rust_name,**kw):
 args=[sys.executable,str(HELP/script),str(OUT/native_name),str(OUT/rust_name),'--report',str(OUT/'failures.json')]
 if kw.get('corpus'):args+=['--corpus']
 result=json.loads(subprocess.check_output(args,cwd=OUT));assert not result['mismatches'];return result
reports={}
for name,generator,count in [('expanded','binder_expanded_cases.exs',16128),('context','binder_context_cases.exs',40)]:
 native(generator,name+'.jsonl');native('binder_oracle.exs',name+'.native.jsonl',name+'.jsonl');rust('binder_probe',name+'.rust.jsonl',name+'.jsonl')
 reports[name]=comparison('verify_binder.py',name+'.native.jsonl',name+'.rust.jsonl');assert reports[name]['outcomes']==count
for direction in ['ab','ba']:
 name='atom_'+direction
 subprocess.run(['elixir',str(HELP/'binder_atom_order.exs'),direction,str(OUT/(name+'.jsonl')),str(OUT/(name+'.native.jsonl'))],cwd=OUT,check=True)
 rust('binder_probe',name+'.rust.jsonl',name+'.jsonl');reports[name]=comparison('verify_binder.py',name+'.native.jsonl',name+'.rust.jsonl');assert reports[name]['outcomes']==1
manifest=json.loads((ROOT/'.cache/core/report.json').read_text())['cases'];assert len(manifest)==327
for row in manifest:retain(row['core_json'])
(OUT/'corpus.jsonl').write_text(''.join(json.dumps({'id':r['id'],'core_json':r['core_json']})+'\n'for r in manifest))
native('corpus_binder_oracle.exs','corpus.native.jsonl','corpus.jsonl')
subprocess.run([sys.executable,str(HELP/'binder_inputs.py'),str(OUT/'corpus.native.jsonl'),'--captured','--output',str(OUT/'corpus.same_vm.jsonl')],cwd=OUT,check=True)
rust('corpus_binder_probe','corpus.rust.jsonl','corpus.same_vm.jsonl');reports['corpus']=comparison('verify_binder.py','corpus.native.jsonl','corpus.rust.jsonl',corpus=True);assert reports['corpus']['outcomes']==133138 and reports['corpus']['modules']==571
native('corpus_printer_oracle.exs','printer.corpus.native.jsonl','corpus.same_vm.jsonl');rust('corpus_printer_probe','printer.corpus.rust.jsonl','corpus.same_vm.jsonl');reports['printer_corpus']=comparison('verify_printer.py','printer.corpus.native.jsonl','printer.corpus.rust.jsonl');assert reports['printer_corpus']['modules']==571
for name,generator,count in [('printer_expanded','printer_expanded_cases.exs',243),('printer_long','printer_long_strings.exs',18)]:
 native(generator,name+'.jsonl');rust('printer_probe',name+'.rust.jsonl',name+'.jsonl')
 report=json.loads(subprocess.check_output([sys.executable,str(HELP/'verify_printer.py'),str(OUT/(name+'.rust.jsonl')),'--report',str(OUT/'failures.json')],cwd=OUT));assert report['rows']==count and not report['mismatches'];reports[name]=report
native('printer_modules.exs','printer_modules.jsonl','printer_expanded.jsonl');rust('printer_probe','printer_modules.rust.jsonl','printer_modules.jsonl')
reports['printer_modules']=json.loads(subprocess.check_output([sys.executable,str(HELP/'verify_printer.py'),str(OUT/'printer_modules.rust.jsonl'),'--report',str(OUT/'failures.json')],cwd=OUT));assert reports['printer_modules']['rows']==243 and not reports['printer_modules']['mismatches']
assert before==hashes(),'source changed during pattern verification'
assert all(digest(Path(path))==value for path,value in proof_inputs.items()),'proof input changed during verification'
inputs=[*OUT.glob('*.jsonl'),*sorted((OUT/'data').rglob('*.json')),*[Path(r['core_json'])for r in manifest]]
report=dict(complete_captured_pattern_binding=True,complete_captured_core_producer=True,complete_module_inference=False,implementation_sha256=before,inputs_sha256={str(p):digest(p)for p in inputs},gates=reports,unsupported=['malformed private Core assertion payloads','arbitrary paragraph layout wider than100000','arbitrary untrusted output expansion','full module lifecycle and guard traversal'])
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(reports))
