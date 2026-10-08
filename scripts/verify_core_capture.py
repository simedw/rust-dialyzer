#!/usr/bin/env python3
"""Regenerate exact labeled Cerl captures from original compiler input."""
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/core_capture';OUT.mkdir(parents=True,exist_ok=True)
manifest=ROOT/'.cache/core-v6/report.json'
cases=json.loads(manifest.read_text())['cases'];assert len(cases)==327
files=list((ROOT/'src').rglob('*.rs'))+[ROOT/p for p in ['Cargo.toml','Cargo.lock','scripts/core_capture_probes/native_core_capture.exs','scripts/core_capture_probes/native_core_capture_corpus.exs','scripts/core_export.exs']]+[Path(__file__).resolve(),manifest]+[Path(c['core_json'])for c in cases]
beams=subprocess.check_output(['elixir','-e','Enum.each([:cerl,:cerl_trees],fn m->IO.puts(:code.which(m))end)'],cwd=ROOT,text=True).splitlines();files.extend(map(Path,beams))
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):sha(p)for p in files}
before=hashes();subprocess.run(['cargo','build','--offline','--release','--bin','core_capture_probe'],cwd=ROOT,check=True)
binary=ROOT/'target/release/core_capture_probe';binary_hash=sha(binary)
synthetic_path=OUT/'synthetic.jsonl'
subprocess.run(['elixir',str(ROOT/'scripts/core_capture_probes/native_core_capture.exs'),str(ROOT),str(synthetic_path)],cwd=ROOT,check=True)
synthetic_data=synthetic_path.read_bytes();synthetic=[json.loads(x)for x in synthetic_data.splitlines()];assert len(synthetic)==60
corpus=[{'id':c['id'],'core_json':c['core_json'],'initial':0,'native_json':str(OUT/f'native-{i}.json')}for i,c in enumerate(cases)]
corpus_path=OUT/'corpus.jsonl';corpus_path.write_text(''.join(json.dumps(r)+'\n'for r in corpus));corpus_data=corpus_path.read_bytes()
native=[json.loads(x)for x in subprocess.check_output(['elixir',str(ROOT/'scripts/core_capture_probes/native_core_capture_corpus.exs'),str(ROOT)],cwd=ROOT,input=corpus_data).splitlines()]
assert len(native)==327 and [v['id']for v in native]==[r['id']for r in corpus]
inputs={str(p):sha(p)for p in [synthetic_path,corpus_path,binary]+[Path(r['native_json'])for r in corpus]}
def verify(data,rows):
 results=[json.loads(x)for x in subprocess.check_output([str(binary)],input=data,cwd=ROOT).splitlines()]
 assert len(results)==len(rows)
 failures=[{'id':row['id'],'rust':r}for row,r in zip(rows,results)if not r.get('matched')or row['id']!=r.get('id')]
 assert not failures,json.dumps(failures[:3])
 return results
synth=verify(synthetic_data,synthetic);actual=verify(corpus_data,corpus)
assert [v['next_label']for v in actual]==[v['next_label']for v in native]
assert before==hashes();assert binary_hash==sha(binary);assert all(sha(Path(p))==h for p,h in inputs.items())
report={'synthetic_cases':len(synth),'compiler_groups':len(actual),'modules':sum(v['modules']for v in actual),'node_occurrences':sum(v['nodes']for v in actual),'reannotated_occurrences':sum(v['reannotated']for v in actual),'proof':'canonical ETF byte equality and original-runtime capture ordering','complete_captured_core_labels':True,'complete_module_inference':False,'implementation_sha256':before,'inputs_sha256':inputs}
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items()if k not in ['implementation_sha256','inputs_sha256']}))
