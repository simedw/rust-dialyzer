#!/usr/bin/env python3
"""Paired serial-atoms versus parallel-atoms fused benchmark with complete output parity."""
import argparse,gzip,hashlib,json,os,re,statistics,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('root',type=Path);p.add_argument('control_binary',type=Path);p.add_argument('binary',type=Path);p.add_argument('inputs',type=Path);p.add_argument('output',type=Path)
p.add_argument('--rounds',type=int,default=5);p.add_argument('--prior',type=Path,required=True);p.add_argument('--options-root',type=Path,required=True)
a=p.parse_args();assert a.rounds>0;a.output.mkdir(parents=True,exist_ok=False)
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
provenance={str(f):digest(f) for f in [a.control_binary,a.binary,a.prior,Path(__file__),a.root/'Cargo.toml',a.root/'Cargo.lock']}
for f in (a.root/'src').rglob('*.rs'):provenance[str(f)]=digest(f)
(a.output/'initial-provenance.json').write_text(json.dumps({'control_commit':'0503224ff35ded474603fe892b167db0fcd9554f','sha256':provenance},indent=2)+'\n')
rows=[]
for workload in ['phoenix','elixir']:
 original=a.inputs/f'{workload}-native-manifest.json';options=a.options_root/workload/'options.json'
 for f in [original,options]:provenance[str(f)]=digest(f)
 source=json.loads(original.read_text())
 for key in ['dialyzer_typesig_beam','dialyzer_contracts_beam']:
  f=Path(source[key]);provenance[str(f)]=digest(f)
 for beam in source['beam_files']:provenance[beam]=digest(Path(beam))
 manifest=a.output/f'{workload}-manifest.json';manifest.write_text(json.dumps({**source,'core_emission':'fused'},indent=2)+'\n')
 (a.output/'initial-provenance.json').write_text(json.dumps({'control_commit':'0503224ff35ded474603fe892b167db0fcd9554f','sha256':provenance},indent=2)+'\n')
 reference=None
 for repeat in range(a.rounds+1):
  for mode in (['serial_atoms','parallel_atoms'] if repeat%2==0 else ['parallel_atoms','serial_atoms']):
   binary=a.control_binary if mode=='serial_atoms' else a.binary
   name=f'{workload}-{repeat}-{mode}';start=time.perf_counter()
   run=subprocess.run(['/usr/bin/time','-l',str(binary),str(a.prior),str(manifest),str(options)],capture_output=True,env={**os.environ,'PATH':'/usr/bin:/bin','RUST_BACKTRACE':'1'},timeout=600)
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
(a.output/'provenance.json').write_text(json.dumps({'control_commit':'0503224ff35ded474603fe892b167db0fcd9554f','sha256':provenance},indent=2)+'\n')
summaries=[]
for workload in ['phoenix','elixir']:
 rr=[x for x in rows if x['workload']==workload and not x['warmup']]
 control={x['repeat']:x for x in rr if x['mode']=='serial_atoms'}
 for mode in ['serial_atoms','parallel_atoms']:
  mm=[x for x in rr if x['mode']==mode]
  summaries.append(dict(workload=workload,mode=mode,samples=len(mm),**{k:statistics.median(x[k] for x in mm) for k in ['wall_seconds','cpu_seconds','peak_rss_bytes','load_seconds','analysis_seconds']}))
 changes=[{'repeat':x['repeat'],'wall_relative_change':x['wall_seconds']/control[x['repeat']]['wall_seconds']-1,'load_relative_change':x['load_seconds']/control[x['repeat']]['load_seconds']-1} for x in rr if x['mode']=='parallel_atoms']
 summaries.append(dict(workload=workload,mode='paired_change',pairs=changes,median_wall_relative_change=statistics.median(x['wall_relative_change'] for x in changes),median_load_relative_change=statistics.median(x['load_relative_change'] for x in changes)))
(a.output/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
