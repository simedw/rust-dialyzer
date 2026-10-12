#!/usr/bin/env python3
"""Paired fresh frontend/full analyzer benchmark, with complete output parity."""
import argparse,gzip,hashlib,json,os,re,statistics,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('root',type=Path);p.add_argument('binary',type=Path);p.add_argument('inputs',type=Path);p.add_argument('output',type=Path);p.add_argument('--rounds',type=int,default=5);p.add_argument('--prior',type=Path);p.add_argument('--options-root',type=Path);a=p.parse_args()
a.output.mkdir(parents=True,exist_ok=False)
original=Path('/Users/simedw/Documents/ChatGPT/rust-dialyzer')
prior=a.prior or original/'.cache/phase0/rust-cone/out/prior-library.json'
options_root=a.options_root or original/'reports/head-to-head-2026-10-09'
script=a.root/'prototypes/core_cache/core_cache.exs'
def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
provenance={str(x):digest(x) for x in [a.binary,prior,script,Path(__file__)]}
for directory in ['src','scripts/beam_core_port']:
 for file in (a.root/directory).rglob('*'):
  if file.is_file():provenance[str(file)]=digest(file)
for file in [a.root/'Cargo.toml',a.root/'Cargo.lock',a.root/'scripts/core_export.exs',a.root/'scripts/core_native_export.exs',a.root/'scripts/compiler_atom_primitives.exs']:
 provenance[str(file)]=digest(file)
rows=[]
def run(cmd,name):
 start=time.perf_counter();r=subprocess.run(['/usr/bin/time','-l',*map(str,cmd)],capture_output=True,timeout=600);wall=time.perf_counter()-start
 gzip.open(a.output/(name+'.stdout.gz'),'wb').write(r.stdout)
 (a.output/(name+'.stderr')).write_bytes(r.stderr)
 if r.returncode: raise RuntimeError((name,r.returncode,r.stderr.decode()[-3000:],r.stdout.decode()[-1000:]))
 value=json.loads(r.stdout.splitlines()[-1])
 stderr=r.stderr.decode();rss=re.search(r'(\d+)\s+maximum resident set size',stderr)
 cpu=re.search(r'([\d.]+)\s+real\s+([\d.]+)\s+user\s+([\d.]+)\s+sys',stderr)
 return value,dict(wall_seconds=wall,peak_rss_bytes=int(rss[1]) if rss else None,cpu_seconds=float(cpu[2])+float(cpu[3]) if cpu else None)
def semantic(value):return {key:value[key] for key in ['status','modules','public_warning_count','warnings','signatures']}
for workload in ['phoenix','elixir']:
 beams=a.root/f'reports/beam-core-port-2026-10-11/evidence/{workload}-beams.txt'
 options=options_root/workload/'options.json'
 native_manifest=a.inputs/f'{workload}-native-manifest.json'
 for file in [beams,options,native_manifest]:provenance[str(file)]=digest(file)
 context_files=json.loads(native_manifest.read_text())
 for key in ['dialyzer_typesig_beam','dialyzer_contracts_beam']:
  file=Path(context_files[key]);provenance[str(file)]=digest(file)
 for beam in beams.read_text().splitlines():provenance[beam]=digest(Path(beam))
 reference=None
 for repeat in range(a.rounds+1):
  pair={}
  for mode in (['compiler','native'] if repeat%2==0 else ['native','compiler']):
   name=f'{workload}-{repeat}-{mode}'
   frontend=None
   if mode=='compiler':
    cache=a.output/(name+'-cache');manifest=a.output/(name+'-manifest.json')
    export,frontend=run(['elixir',script,a.root,beams,cache,manifest],name+'-export')
    assert export['compiled']==len(beams.read_text().splitlines()) and export['reused']==0
   else:manifest=native_manifest
   output,metric=run([a.binary,prior,manifest,options],name+'-analysis')
   observed=semantic(output)
   if reference is None:reference=observed
   assert observed==reference,(name,'full warnings/signatures mismatch')
   total=metric['wall_seconds']+(frontend['wall_seconds'] if frontend else 0)
   peak=max(x['peak_rss_bytes'] for x in [metric,frontend] if x and x['peak_rss_bytes'] is not None)
   cpu=sum(x['cpu_seconds'] for x in [metric,frontend] if x and x['cpu_seconds'] is not None)
   row=dict(workload=workload,repeat=repeat,warmup=repeat==0,mode=mode,total_wall_seconds=total,total_cpu_seconds=cpu,peak_rss_bytes=peak,analyzer=metric,compiler_export=frontend,load_seconds=output['load_seconds'],analysis_seconds=output['analysis_seconds'],frontend=output['frontend'],warnings=output['public_warning_count'],signatures=len(output['signatures']),exact_parity=True)
   rows.append(row);pair[mode]=total
   if mode=='compiler':
    import shutil
    shutil.rmtree(cache)
   (a.output/'measurements.json').write_text(json.dumps(rows,indent=2)+'\n')
  print(json.dumps(dict(workload=workload,repeat=repeat,seconds=pair)),flush=True)
 summaries=[]
 for mode in ['compiler','native']:
  samples=[r['total_wall_seconds'] for r in rows if r['workload']==workload and r['mode']==mode and not r['warmup']]
  summaries.append(dict(mode=mode,samples=samples,median_seconds=statistics.median(samples)))
 (a.output/f'{workload}-summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
assert all(digest(Path(path))==sha for path,sha in provenance.items()),'inputs/source/binary changed'
(a.output/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
