#!/usr/bin/env python3
"""Compiler-valid exhaustive bounded-cycle source proof, with immutable input hashes."""
import argparse, hashlib, json, os, pathlib, subprocess
BASE=pathlib.Path(__file__).resolve().parents[1]
sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--environment',required=True);p.add_argument('--probe',required=True);p.add_argument('--out',required=True);a=p.parse_args()
 out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True)
 env=dict(os.environ,ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28',ASDF_RUST_VERSION='1.92.0')
 sources={str(f.relative_to(BASE)):sha(f) for f in sorted((BASE/'src').rglob('*.rs'))}
 for f in ['scripts/preparation_probes/preparation_cycles.exs','scripts/verify_preparation_cycles.py','scripts/verify_compiled_definitions.py','scripts/preparation_probes/compiled_definitions_oracle.exs','scripts/preparation_probes/compiled_definitions_compare.exs','scripts/core_export.exs','scripts/forms_runtime.exs','Cargo.lock','Cargo.toml','scripts/native_types.exs']:sources[f]=sha(BASE/f)
 environment=str(pathlib.Path(a.environment).resolve());eh=sha(environment);probe=str(pathlib.Path(a.probe).resolve());ph=sha(probe)
 subprocess.run(['elixir',str(BASE/'scripts/preparation_probes/preparation_cycles.exs'),environment,str(out/'inputs')],cwd=BASE,env=env,check=True)
 subprocess.run(['python3',str(BASE/'scripts/verify_compiled_definitions.py'),'--requests',str(out/'inputs'/'requests.jsonl'),'--probe',probe,'--out',str(out/'proof'),'--expected-cases','20','--expected-records','0','--expected-contracts','531'],cwd=BASE,env=env,check=True)
 for f,h in sources.items():
  if sha(BASE/f)!=h:raise ValueError('source changed:'+f)
 if sha(environment)!=eh or sha(probe)!=ph:raise ValueError('input/binary changed')
 result=dict(cases=20,matches=20,contracts=531,exhaustive_three_vertex_graphs=512,additional_source_fixtures=19,implementation_sha256=sources,inputs_sha256={environment:eh,probe:ph},source_sha256=sources,environment_sha256=eh,probe_sha256=ph,artifacts_sha256={str(f.relative_to(out)):sha(f) for f in sorted(out.rglob('*')) if f.is_file()})
 (out/'report.json').write_text(json.dumps(result,indent=2)+'\n');print({k:v for k,v in result.items() if not k.endswith('sha256')})
if __name__=='__main__':main()
