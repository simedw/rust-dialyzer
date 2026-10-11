#!/usr/bin/env python3
"""Run only under the shared lock; binaries and sources must remain frozen."""
import argparse,hashlib,json,pathlib,subprocess,time
p=argparse.ArgumentParser();p.add_argument('binary',type=pathlib.Path);p.add_argument('stage',choices=['forms','clean']);p.add_argument('iterations',type=int);p.add_argument('output',type=pathlib.Path);p.add_argument('manifests',type=pathlib.Path,nargs='+');a=p.parse_args()
results=[]
for manifest in a.manifests:
 commands={'rust':[str(a.binary),a.stage,str(a.iterations),str(manifest)],'compiler':['elixir','scripts/beam_core_port/bench.exs',a.stage,str(a.iterations),str(manifest)]}
 row={'manifest':str(manifest),'manifest_sha256':hashlib.sha256(manifest.read_bytes()).hexdigest()}
 for name in ['compiler','rust']:
  begin=time.monotonic();r=subprocess.run(commands[name],capture_output=True,text=True)
  if r.returncode:raise SystemExit(r.stderr)
  row[name]=json.loads(r.stdout);row[name]['process_wall_seconds']=time.monotonic()-begin
 row['compiler_over_rust_ratio']=row['compiler']['median_ms']/row['rust']['median_ms'];results.append(row)
 report={'stage':a.stage,'native_binary_sha256':hashlib.sha256(a.binary.read_bytes()).hexdigest(),'compiler_script_sha256':hashlib.sha256(pathlib.Path('scripts/beam_core_port/bench.exs').read_bytes()).hexdigest(),'output_lifetime':'Each module result is discarded before the next module on both sides.','results':results,'scope':'Warm resident-input sequential frontend stages. IO, VM startup, source compilation, transport and analyzer excluded.'}
 a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(row),flush=True)
