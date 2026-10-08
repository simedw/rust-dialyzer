import argparse,hashlib,json,time,subprocess,statistics
from pathlib import Path
b=Path(__file__).resolve().parents[1]
repo=b.parents[1]
p=argparse.ArgumentParser()
p.add_argument('--binary-root',type=Path,default=b)
a=p.parse_args()
s=a.binary_root.resolve()
out=b/'corpus-native/frontend-timing';out.mkdir(parents=True,exist_ok=True)
rows=[json.loads(x) for x in (b/'harness/corpus/oracle-row.jsonl').read_text().splitlines()]
measurements={'native_debug_frontend':[],'existing_oracle_bridge':[]}
paths_file=out/'paths.txt'
paths_file.write_text(''.join(row['beam_path']+'\n' for row in rows))
for repeat in range(3):
 t=time.perf_counter()
 for row in rows:
  beam=row['beam_path'];name=row['module'];seed=out/(name+'.seed.json');core=out/(name+'.core.json')
  subprocess.run([str(s/'input/target/debug/native-beam-seed'),beam,str(seed)],check=True,stdout=subprocess.DEVNULL)
  subprocess.run([str(s/'adapter/target/debug/adapt-beam'),beam,str(seed),str(core)],check=True,stdout=subprocess.DEVNULL)
 measurements['native_debug_frontend'].append((time.perf_counter()-t)*1000)
 t=time.perf_counter()
 p=subprocess.run(['elixir',str(b/'run/frontend_timing.exs'),str(repo),str(paths_file)],stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,text=True)
 if p.returncode:raise RuntimeError(p.stderr)
 measurements['existing_oracle_bridge'].append((time.perf_counter()-t)*1000)
 print(repeat, {k:round(v[-1],1) for k,v in measurements.items()},flush=True)
report={'cases':13,'measurements_ms':measurements,'median_ms':{k:statistics.median(v) for k,v in measurements.items()},'scope':'frontend only; warm filesystem cache; debug native binaries; current bridge includes VM startup, dynamic helper compilation and OTP reference-Core validation; no analyzer, incremental or release-speed claim'}
report['sha256']={str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in [Path(__file__),b/'run/frontend_timing.exs',s/'input/target/debug/native-beam-seed',s/'adapter/target/debug/adapt-beam']}
(b/'evidence/frontend-timing.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
