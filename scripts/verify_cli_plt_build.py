#!/usr/bin/env python3
"""Compare actual native and Rust PLT builds, warnings and all five tables."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from benchmark_cli_analysis import warnings

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--candidate',type=Path,required=True)
p.add_argument('--out',type=Path,required=True)
p.add_argument('beams',nargs='+',type=Path)
a=p.parse_args()
a.out.mkdir(parents=True,exist_ok=False)
root=Path(__file__).resolve().parents[1]
prior=root/'.cache/corpus/otp/base.plt'
erlang=Path('/Users/simedw/.asdf/installs/erlang/28.3')
elixir=Path('/Users/simedw/.asdf/installs/elixir/1.19.4-otp-28')
env=dict(os.environ)
env['PATH']=f'{erlang}/bin:{elixir}/bin:'+env['PATH']
paths=[prior,a.candidate,*a.beams]
def inventory():
    return {str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
before=inventory()
observed={}
for backend in ['native','rust']:
    command=([str(erlang/'bin/dialyzer'),'-pa',str(elixir/'lib/elixir/ebin')] if backend=='native' else [str(a.candidate)])
    command+=['--build_plt','--get_warnings','--output_plt',str(a.out/(backend+'.plt')),'-Wno_unknown',*map(str,a.beams)]
    run=subprocess.run(command,env=env,capture_output=True,timeout=600)
    (a.out/(backend+'.stdout')).write_bytes(run.stdout)
    (a.out/(backend+'.stderr')).write_bytes(run.stderr)
    assert run.returncode in (0,2),(backend,run.stderr.decode())
    observed[backend]=(run.returncode,'\n'.join(line for line in warnings(run.stdout).splitlines() if not line.startswith(('  Creating PLT','Creating PLT')))) 
assert observed['native']==observed['rust'],'warnings differ'
code='''[native,rust,out]=System.argv(); load=fn path -> plt=:dialyzer_cplt.from_file(String.to_charlist(path)); tables=for i<-1..5,do: :ets.tab2list(elem(plt,i)) |> Enum.sort(); :dialyzer_plt.delete(plt); tables end; a=load.(native); b=load.(rust); exact=Enum.zip(a,b) |> Enum.map(fn {a,b}->a===b end); if not Enum.all?(exact),do: raise("output PLT tables differ: #{inspect(exact)}"); File.write!(out,:json.encode(%{all_five_native_tables_exact: true,table_rows: Enum.map(a,&length/1)}))'''
reader=subprocess.run(['elixir','-e',code,str(a.out/'native.plt'),str(a.out/'rust.plt'),str(a.out/'tables.json')],env=env,capture_output=True,timeout=120)
(a.out/'reader.stderr').write_bytes(reader.stderr)
assert reader.returncode==0,reader.stderr.decode()
assert inventory()==before,'inputs or candidate changed'
report=dict(json.loads((a.out/'tables.json').read_text()),warnings_exact=True,inputs_unchanged=True,input_sha256=before)
(a.out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report),flush=True)
