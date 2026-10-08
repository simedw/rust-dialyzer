#!/usr/bin/env python3
"""Original source Call/Apply controller, with compiler-only Core transport inputs."""
import argparse,hashlib,json,os,pathlib,subprocess,time
BASE=pathlib.Path(__file__).resolve().parents[1]
sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--root',required=True);p.add_argument('--probe',required=True);p.add_argument('--out',required=True);a=p.parse_args();root=pathlib.Path(a.root).resolve();out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=True)
 env=dict(os.environ,ASDF_ERLANG_VERSION='28.3',ASDF_ELIXIR_VERSION='1.19.4-otp-28',ASDF_RUST_VERSION='1.92.0')
 sources={str(f.resolve()):sha(f) for f in sorted((BASE/'src').rglob('*.rs'))}
 for f in ['scripts/native_body_calls.exs','scripts/compare_body_calls.exs','scripts/verify_body_calls.py','Cargo.toml','Cargo.lock','corpus/bifs.json','corpus/dependency_policy.json']:sources[str(BASE/f)]=sha(BASE/f)
 for f in ['scripts/type_codec.exs','scripts/type_wire.exs','scripts/core_export.exs','scripts/dataflow_state_probes/function_state_oracle.exs']:sources[str(root/f)]=sha(root/f)
 beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_dataflow,:dialyzer_contracts,:erl_types,:erl_bif_types,:cerl,:cerl_trees,:dict],fn m->IO.puts(:code.which(m))end)'],cwd=root,env=env,text=True).splitlines()
 for beam in beams:sources[beam]=sha(beam)
 probe=str(pathlib.Path(a.probe).resolve());bh=sha(probe)
 native=out/'native.jsonl';rust=out/'rust.jsonl';report=out/'comparison.json';start=time.monotonic()
 subprocess.run(['elixir',str(BASE/'scripts/native_body_calls.exs'),str(root),str(native)],cwd=BASE,env=env,check=True)
 native_hash=sha(native)
 with native.open('rb') as i,rust.open('wb') as o:subprocess.run([probe],stdin=i,stdout=o,cwd=BASE,env=env,check=True)
 subprocess.run(['elixir',str(BASE/'scripts/compare_body_calls.exs'),str(root),str(native),str(rust),str(report)],cwd=BASE,env=env,check=True)
 r=json.loads(report.read_text())
 if r['cases']!=21750 or r['matches']!=21750 or r['failures']:raise ValueError('unexpected counts or mismatches')
 for f,h in sources.items():
  if sha(f)!=h:raise ValueError('source changed:'+f)
 if sha(native)!=native_hash:raise ValueError('native input changed')
 if sha(probe)!=bh:raise ValueError('binary changed')
 result=dict(cases=r['cases'],matches=r['matches'],native_errors=r['native_errors'],source_sha256=sources,probe_sha256=bh,artifact_sha256={f.name:sha(f) for f in [native,rust,report]},elapsed_seconds=time.monotonic()-start,scope=dict(raw_warnings=True,maps=True,function_state=True,native_errors=True,nested_body_call_frames='complete root evaluated by shared BodyFlow',arbitrary_malformed_contract_signature_arities=False))
 (out/'report.json').write_text(json.dumps(result,indent=2)+'\n');print({k:v for k,v in result.items() if not k.endswith('sha256')})
if __name__=='__main__':main()
