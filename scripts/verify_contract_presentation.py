#!/usr/bin/env python3
"""Exact original Dialyzer contract presentation; shared production form printer."""
import argparse,hashlib,json,os,pathlib,subprocess
BASE=pathlib.Path(__file__).resolve().parents[1]
sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--captures',required=True);p.add_argument('--dependency',required=True);p.add_argument('--probe',required=True);p.add_argument('--out',required=True);a=p.parse_args();out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True)
 env=dict(os.environ,ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28',ASDF_RUST_VERSION='1.92.0')
 sources={str(f.relative_to(BASE)):sha(f) for f in sorted((BASE/'src').rglob('*.rs'))}
 for f in ['scripts/preparation_probes/contract_presentation_gate.exs','scripts/preparation_probes/contract_presentation_corpus.exs','scripts/preparation_probes/contract_presentation_compare.exs','scripts/verify_contract_presentation.py','scripts/core_export.exs','Cargo.lock','Cargo.toml']:sources[f]=sha(BASE/f)
 beam_paths=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_contracts,:erl_types,:io_lib,:io_lib_format,:io_lib_pretty],fn m->IO.puts(:code.which(m))end)'],cwd=BASE,env=env,text=True).splitlines()
 for beam in beam_paths:sources[beam]=sha(beam)
 inputs={str(pathlib.Path(f).resolve()):sha(f) for f in [a.captures,a.dependency]};binary=str(pathlib.Path(a.probe).resolve());bh=sha(binary)
 def run(cmd):subprocess.run(cmd,cwd=BASE,env=env,check=True)
 run(['elixir',str(BASE/'scripts/preparation_probes/contract_presentation_gate.exs'),str(out/'direct')]);run(['elixir',str(BASE/'scripts/preparation_probes/contract_presentation_corpus.exs'),str(pathlib.Path(a.captures).resolve()),str(pathlib.Path(a.dependency).resolve()),str(out/'actual')])
 reports={}
 for d,count in [('direct',62),('actual',9600)]:
  inputs[str(out/d/'requests.jsonl')]=sha(out/d/'requests.jsonl');inputs[str(out/d/'native.jsonl')]=sha(out/d/'native.jsonl')
  with (out/d/'requests.jsonl').open('rb') as src,(out/d/'rust.jsonl').open('wb') as dst:subprocess.run([binary],stdin=src,stdout=dst,cwd=BASE,env=env,check=True)
  run(['elixir',str(BASE/'scripts/preparation_probes/contract_presentation_compare.exs'),str(out/d)]);r=json.loads((out/d/'report.json').read_text())
  if r['cases']!=count or r['matches']!=count:raise ValueError('unexpected proof counts:'+d)
  reports[d]=dict(cases=count,matches=count)
 for f,h in sources.items():
  if sha(BASE/f)!=h:raise ValueError('source changed:'+f)
 for f,h in inputs.items():
  if sha(f)!=h:raise ValueError('input changed:'+f)
 if sha(binary)!=bh:raise ValueError('binary changed')
 artifacts={str(f.relative_to(out)):sha(f) for f in sorted(out.rglob('*')) if f.is_file()}
 r=dict(gates=reports,implementation_sha256=sources,inputs_sha256=inputs|{binary:bh},source_sha256=sources,input_sha256=inputs,binary_sha256={binary:bh},artifact_sha256=artifacts)
 (out/'report.json').write_text(json.dumps(r,indent=2)+'\n');print(reports)
if __name__=='__main__':main()
