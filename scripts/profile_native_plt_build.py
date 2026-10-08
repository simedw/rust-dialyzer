#!/usr/bin/env python3
"""Time a Native PLT build using the same captured request as the Rust profiler."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--request',type=Path,required=True)
p.add_argument('--out',type=Path,required=True)
a=p.parse_args()
a.out.mkdir(parents=True,exist_ok=False)
r=json.loads(a.request.read_text())
assert r['operation']=='plt_build' and not r['priors']
assert not r['get_warnings'] and r['use_contracts'] and not r['solvers']
erlang=Path('/Users/simedw/.asdf/installs/erlang/28.3')
env=dict(os.environ)
env['PATH']=str(erlang/'bin')+':/Users/simedw/.asdf/installs/elixir/1.19.4-otp-28/bin:'+env['PATH']
paths=[Path(__file__).resolve(),a.request,*map(Path,r['files'])]
paths.extend(x for x in (erlang/'lib').rglob('*') if x.is_file() and x.suffix in ['.beam','.erl'])
def inventory():
 return {str(x):hashlib.sha256(x.read_bytes()).hexdigest() for x in sorted(set(paths))}
before=inventory()
command=[str(erlang/'bin/dialyzer'),'--build_plt','--output_plt',str(a.out/'native.plt')]
command.extend('-W'+option for option in r['warning_options'])
command.extend(r['files'])
started=time.perf_counter()
run=subprocess.run(command,env=env,capture_output=True,timeout=600)
elapsed=time.perf_counter()-started
(a.out/'stdout').write_bytes(run.stdout)
(a.out/'stderr').write_bytes(run.stderr)
assert run.returncode==0,run.stderr.decode()
assert inventory()==before,'Native runtime or inputs changed'
report=dict(seconds=elapsed,exit_status=run.returncode,rounds=1,warmup=False,inputs_unchanged=True,scope='Complete Native CLI PLT build on the same target BEAMs with contracts and no_unknown; warnings collection disabled by default, matching the captured public_run request.')
(a.out/'provenance.json').write_text(json.dumps(before,indent=2)+'\n')
(a.out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report),flush=True)
