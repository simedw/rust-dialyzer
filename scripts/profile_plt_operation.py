#!/usr/bin/env python3
"""Time an actual captured PLT build and compare all five Native-readable tables."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--candidate', type=Path, required=True)
p.add_argument('--operations', type=Path, required=True)
p.add_argument('--reference-plt', type=Path, required=True)
p.add_argument('--out', type=Path, required=True)
a = p.parse_args()
a.out.mkdir(parents=True, exist_ok=False)
records = [json.loads(line) for line in a.operations.read_text().splitlines()]
rows = [record['request'] for record in records if 'request' in record]
requests = [r for r in rows if r['operation'] == 'plt_build' and any('/erlang.beam' in f for f in r.get('files', []))]
assert requests
request = dict(requests[-1])
request['output'] = str(a.out / 'candidate.plt')
paths = [Path(__file__).resolve(), a.candidate, a.operations, a.reference_plt, *map(Path, request['files'])]
root = a.candidate.resolve().parents[2]
for directory in ['src', 'scripts', 'corpus']:
    paths.extend(x for x in (root / directory).rglob('*') if x.is_file())
def inventory():
    return {str(x): hashlib.sha256(x.read_bytes()).hexdigest() for x in sorted(set(paths))}
before = inventory()
env = dict(os.environ)
env.pop('RUST_DIALYZER_TRACE', None)
env['PATH'] = '/Users/simedw/.asdf/installs/erlang/28.3/bin:/Users/simedw/.asdf/installs/elixir/1.19.4-otp-28/bin:' + env['PATH']
(a.out / 'request.json').write_text(json.dumps(request, indent=2) + '\n')
started = time.perf_counter()
run = subprocess.run([str(a.candidate)], input=(json.dumps(request) + '\n').encode(), env=env, capture_output=True, timeout=3600)
elapsed = time.perf_counter() - started
(a.out / 'stdout').write_bytes(run.stdout)
(a.out / 'stderr').write_bytes(run.stderr)
assert run.returncode == 0, run.stderr.decode()[-2000:]
response = json.loads(run.stdout)
assert response['status'] == 'ok', response
reader = '''[reference,candidate,out]=System.argv(); load=fn path -> plt=:dialyzer_cplt.from_file(String.to_charlist(path)); tables=for i<-1..5,do: :ets.tab2list(elem(plt,i)) |> Enum.sort(); :dialyzer_plt.delete(plt); tables end; a=load.(reference); b=load.(candidate); exact=Enum.zip(a,b) |> Enum.map(fn {a,b}->a===b end); if not Enum.all?(exact),do: raise("PLT tables differ: #{inspect(exact)}"); File.write!(out,:json.encode(%{all_five_native_tables_exact: true, table_rows: Enum.map(a,&length/1)}))'''
check = subprocess.run(['elixir', '-e', reader, str(a.reference_plt), request['output'], str(a.out / 'tables.json')], env=env, capture_output=True, timeout=120)
(a.out / 'reader.stderr').write_bytes(check.stderr)
assert check.returncode == 0, check.stderr.decode()
assert inventory() == before, 'implementation or inputs changed during measurement'
report = dict(json.loads((a.out / 'tables.json').read_text()), seconds=elapsed, response=response, inputs_unchanged=True, rounds=1, warmup=False, scope='One untraced complete public PLT build over actual erlang.beam and the Original merge fixture; no repeated-run speed claim.')
(a.out / 'provenance.json').write_text(json.dumps(before, indent=2) + '\n')
(a.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report), flush=True)
