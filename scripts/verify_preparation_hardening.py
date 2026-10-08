#!/usr/bin/env python3
"""Native-source preparation boundary lever; no native algebra fallback in Rust."""
import argparse,hashlib,json,os,pathlib,subprocess,sys,time
BASE=pathlib.Path(__file__).resolve().parents[1]

def sha(path):return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()
def run(cmd,env):subprocess.run(cmd,cwd=BASE,env=env,check=True)
def main():
 p=argparse.ArgumentParser();p.add_argument('--environment',required=True);p.add_argument('--out',required=True);p.add_argument('--probe',required=True);p.add_argument('--format-probe',required=True);p.add_argument('--corpus-requests');a=p.parse_args()
 out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True)
 env=dict(os.environ,ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28',ASDF_RUST_VERSION='1.92.0')
 source={str(p.relative_to(BASE)):sha(p) for p in sorted((BASE/'src').rglob('*.rs'))}
 for name in ['scripts/verify_preparation_hardening.py','scripts/verify_compiled_definitions.py','scripts/preparation_probes/preparation_boundaries.exs','scripts/preparation_probes/preparation_import_boundaries.exs','scripts/preparation_probes/compiled_definitions_oracle.exs','scripts/preparation_probes/compiled_definitions_compare.exs','scripts/preparation_probes/form_format_gate.exs','scripts/preparation_probes/form_format_compare.exs','scripts/core_export.exs','scripts/forms_runtime.exs','scripts/native_types.exs','Cargo.toml','Cargo.lock']:source[name]=sha(BASE/name)
 binaries={str(pathlib.Path(x).resolve()):sha(x) for x in [a.probe,a.format_probe]};environment_hash=sha(a.environment)
 for script,subdir in [('scripts/preparation_probes/preparation_boundaries.exs','constraints'),('scripts/preparation_probes/preparation_import_boundaries.exs','imports')]:run(['elixir',str(BASE/script),str(pathlib.Path(a.environment).resolve()),str(out/subdir)],env)
 requests=[]
 for d,n in [('constraints',19),('imports',7)]:
  rows=[json.loads(x) for x in (out/d/'requests.jsonl').read_text().splitlines()]
  if len(rows)!=n:raise ValueError(f'{d} generation count changed')
  requests+=rows
 req=out/'requests.jsonl';req.write_text(''.join(json.dumps(r)+'\n' for r in requests))
 run([sys.executable,str(BASE/'scripts/verify_compiled_definitions.py'),'--requests',str(req),'--probe',str(pathlib.Path(a.probe).resolve()),'--out',str(out/'preparation'),'--expected-cases','26','--expected-records','0','--expected-contracts','8'],env)
 native=[json.loads(x) for x in (out/'preparation'/'native.jsonl').read_text().splitlines()]
 # Success counts reject a silently dropped source spec. Worker exits are evidence,
 # separately classified from native throws caught by original helpers.
 kinds={};boundaries={}
 for r in native:
  k=r.get('kind',r['status']);kinds[k]=kinds.get(k,0)+1
  k=r.get('native_failure_boundary','returned');boundaries[k]=boundaries.get(k,0)+1
 run(['elixir',str(BASE/'scripts/preparation_probes/form_format_gate.exs'),str(out/'format')],env)
 with (out/'format'/'requests.jsonl').open('rb') as src,(out/'format'/'rust.jsonl').open('wb') as dst:subprocess.run([str(pathlib.Path(a.format_probe).resolve())],stdin=src,stdout=dst,cwd=BASE,env=env,check=True)
 run(['elixir',str(BASE/'scripts/preparation_probes/form_format_compare.exs'),str(out/'format')],env)
 fmt=json.loads((out/'format'/'report.json').read_text());assert fmt['cases']==90 and fmt['matches']==90
 if a.corpus_requests:run([sys.executable,str(BASE/'scripts/verify_compiled_definitions.py'),'--requests',a.corpus_requests,'--probe',str(pathlib.Path(a.probe).resolve()),'--out',str(out/'corpus')],env)
 if environment_hash!=sha(a.environment):raise ValueError('source environment mutated')
 for f,h in source.items():
  if sha(BASE/f)!=h:raise ValueError('source mutated: '+f)
 for f,h in binaries.items():
  if sha(f)!=h:raise ValueError('binary mutated: '+f)
 artifacts={str(p.relative_to(out)):sha(p) for p in sorted(out.rglob('*')) if p.is_file()}
 report=dict(preparation_cases=26,preparation_matches=26,source_contracts=8,format_cases=90,format_matches=90,native_outcomes=kinds,native_failure_boundaries=boundaries,native_worker_timeout_ms=60000,implementation_sha256=source,inputs_sha256=binaries|{str(pathlib.Path(a.environment).resolve()):environment_hash},source_sha256=source,binary_sha256=binaries,environment_sha256=environment_hash,artifacts_sha256=artifacts,unsupported_boundaries=['arbitrary malformed imported metadata headers/table keys','non atom/integer native variable names','non-default io_lib formatting controls'])
 (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if not k.endswith('sha256')}))
if __name__=='__main__':main()
