#!/usr/bin/env python3
"""Regenerate exact source-owned preparation scope/header/record boundaries."""
import argparse,hashlib,json,os,pathlib,subprocess,sys
BASE=pathlib.Path(__file__).resolve().parents[1]
sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--environment',required=True);p.add_argument('--probe',required=True);p.add_argument('--out',required=True);p.add_argument('--corpus-requests');a=p.parse_args()
 out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True)
 env=dict(os.environ,ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28',ASDF_RUST_VERSION='1.92.0')
 scripts=['scripts/preparation_probes/preparation_scope_boundaries.exs','scripts/preparation_probes/preparation_malformed_boundaries.exs','scripts/preparation_probes/preparation_record_boundaries.exs','scripts/preparation_probes/preparation_table_boundaries.exs','scripts/preparation_probes/compiled_definitions_oracle.exs','scripts/preparation_probes/compiled_definitions_compare.exs','scripts/verify_compiled_definitions.py','scripts/verify_preparation_scope.py','scripts/core_export.exs','scripts/forms_runtime.exs','scripts/native_types.exs','Cargo.toml','Cargo.lock']
 sources={str(p.relative_to(BASE)):sha(p) for p in sorted((BASE/'src').rglob('*.rs'))};sources.update({s:sha(BASE/s) for s in scripts});binary=str(pathlib.Path(a.probe).resolve());bh=sha(binary);original=str(pathlib.Path(a.environment).resolve());eh=sha(original)
 def run(cmd):subprocess.run(cmd,cwd=BASE,env=env,check=True)
 for script,subdir in [('scripts/preparation_probes/preparation_scope_boundaries.exs','scope'),('scripts/preparation_probes/preparation_malformed_boundaries.exs','headers'),('scripts/preparation_probes/preparation_record_boundaries.exs','records')]:run(['elixir',str(BASE/script),original,str(out/subdir)])
 run(['elixir',str(BASE/'scripts/preparation_probes/preparation_table_boundaries.exs'),original,str(out/'headers'/'unused_scalar.core.json'),str(out/'scope'/'redirect_scalar.core.json'),str(out/'tables')])
 requests=[]
 for subdir,count in [('scope',10),('headers',16),('records',28),('tables',6)]:
  rows=[json.loads(x) for x in (out/subdir/'requests.jsonl').read_text().splitlines()]
  if len(rows)!=count:raise ValueError('generation count differs:'+subdir)
  requests+=rows
 request=out/'requests.jsonl';request.write_text(''.join(json.dumps(r)+'\n' for r in requests))
 run([sys.executable,str(BASE/'scripts/verify_compiled_definitions.py'),'--requests',str(request),'--probe',binary,'--out',str(out/'proof'),'--expected-cases','60','--expected-records','0','--expected-contracts','41'])
 ns=[json.loads(x) for x in (out/'proof'/'native.jsonl').read_text().splitlines()];kinds={};boundaries={}
 for r in ns:
  k=r.get('kind',r['status']);kinds[k]=kinds.get(k,0)+1;k=r.get('native_failure_boundary','returned');boundaries[k]=boundaries.get(k,0)+1
 if a.corpus_requests:run([sys.executable,str(BASE/'scripts/verify_compiled_definitions.py'),'--requests',a.corpus_requests,'--probe',binary,'--out',str(out/'corpus')])
 if bh!=sha(binary) or eh!=sha(original):raise ValueError('binary/environment changed')
 for f,h in sources.items():
  if sha(BASE/f)!=h:raise ValueError('source changed:'+f)
 artifacts={str(p.relative_to(out)):sha(p) for p in sorted(out.rglob('*')) if p.is_file()}
 report=dict(cases=60,matches=60,records=0,contracts=41,native_outcomes=kinds,native_failure_boundaries=boundaries,native_worker_timeout_ms=60000,implementation_sha256=sources,inputs_sha256={binary:bh,original:eh},binary_sha256={binary:bh},source_sha256=sources,environment_sha256={original:eh},artifacts_sha256=artifacts,unverified=['non-atom defining-module cache and nominal identities','native variables/parameter keys outside atom/integer','malformed outer compiler metadata/frontend shape','failure precedence among multiple malformed library entries across VM atom-order provenance'])
 (out/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if not k.endswith('sha256')}))
if __name__=='__main__':main()
