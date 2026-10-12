#!/usr/bin/env python3
"""Compare native reference, direct typed emission and fused cleanup with exact outputs."""
import argparse,gzip,hashlib,json,os,re,statistics,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('root',type=Path);p.add_argument('binary',type=Path);p.add_argument('inputs',type=Path);p.add_argument('output',type=Path)
p.add_argument('--rounds',type=int,default=5);p.add_argument('--modes',nargs='+',choices=['reference','direct','fused'],default=['reference','direct','fused'])
p.add_argument('--prior',type=Path,required=True);p.add_argument('--options-root',type=Path,required=True)
a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
provenance={str(f):digest(f) for f in [a.binary,a.prior,Path(__file__),a.root/'Cargo.toml',a.root/'Cargo.lock']}
for f in (a.root/'src').rglob('*.rs'):provenance[str(f)]=digest(f)
rows=[]
for workload in ['phoenix','elixir']:
 original=a.inputs/f'{workload}-native-manifest.json';options=a.options_root/workload/'options.json'
 for f in [original,options]:provenance[str(f)]=digest(f)
 source=json.loads(original.read_text())
 for name in ['dialyzer_typesig_beam','dialyzer_contracts_beam']:
  f=Path(source[name]);provenance[str(f)]=digest(f)
 for b in source['beam_files']:provenance[b]=digest(Path(b))
 manifests={}
 for mode in a.modes:
  path=a.output/f'{workload}-{mode}-manifest.json'
  path.write_text(json.dumps({**source,'core_emission':mode},indent=2)+'\n');manifests[mode]=path
 reference=None
 for repeat in range(a.rounds+1):
  modes=a.modes[repeat%len(a.modes):]+a.modes[:repeat%len(a.modes)]
  if repeat%2:modes=list(reversed(modes))
  for mode in modes:
   name=f'{workload}-{repeat}-{mode}';start=time.perf_counter()
   run=subprocess.run(['/usr/bin/time','-l',str(a.binary),str(a.prior),str(manifests[mode]),str(options)],capture_output=True,env={**os.environ,'PATH':'/usr/bin:/bin'},timeout=600)
   wall=time.perf_counter()-start
   (a.output/(name+'.stdout.gz')).write_bytes(gzip.compress(run.stdout));(a.output/(name+'.stderr')).write_bytes(run.stderr)
   assert run.returncode==0,(name,run.stderr[-1500:])
   out=json.loads(run.stdout);value={k:out[k] for k in ['status','modules','public_warning_count','warnings','signatures']}
   if reference is None:reference=value
   assert value==reference,(name,'complete output mismatch')
   stderr=run.stderr.decode();rss=re.search(r'(\d+)\s+maximum resident set size',stderr);cpu=re.search(r'([\d.]+)\s+real\s+([\d.]+)\s+user\s+([\d.]+)\s+sys',stderr)
   row=dict(workload=workload,repeat=repeat,warmup=repeat==0,mode=mode,wall_seconds=wall,cpu_seconds=float(cpu[2])+float(cpu[3]),peak_rss_bytes=int(rss[1]),frontend=out['frontend'],load_seconds=out['load_seconds'],analysis_seconds=out['analysis_seconds'],warnings=out['public_warning_count'],signatures=len(out['signatures']),exact_parity=True)
   rows.append(row);(a.output/'measurements.json').write_text(json.dumps(rows,indent=2)+'\n')
   print(json.dumps({k:row[k] for k in ['workload','repeat','mode','wall_seconds','peak_rss_bytes']}),flush=True)
assert all(digest(Path(f))==sha for f,sha in provenance.items()),'source/input/binary changed during run'
(a.output/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
summaries=[]
for workload in ['phoenix','elixir']:
 for mode in a.modes:
  rr=[x for x in rows if x['workload']==workload and x['mode']==mode and not x['warmup']]
  summaries.append(dict(workload=workload,mode=mode,samples=len(rr),**{k:statistics.median(x[k] for x in rr) for k in ['wall_seconds','cpu_seconds','peak_rss_bytes','load_seconds','analysis_seconds']}))
(a.output/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
