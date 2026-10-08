#!/usr/bin/env python3
"""Compare public analysis with an unused malformed contract in a genuine PLT."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from benchmark_cli_analysis import warnings

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--candidate', type=Path, required=True)
p.add_argument('--out', type=Path, required=True)
p.add_argument('beams', type=Path, nargs='+')
a = p.parse_args()
a.out.mkdir(parents=True, exist_ok=False)
root = Path(__file__).resolve().parents[1]
prior = root / '.cache/corpus/otp/base.plt'
otp = Path('/Users/simedw/.asdf/installs/erlang/28.3')
elixir = Path('/Users/simedw/.asdf/installs/elixir/1.19.4-otp-28')
env = dict(os.environ)
env['PATH'] = f'{otp}/bin:{elixir}/bin:' + env['PATH']
paths = [Path(__file__), prior, a.candidate, *a.beams]
inventory = lambda: {str(x): hashlib.sha256(x.read_bytes()).hexdigest() for x in paths}
before = inventory()
mutated = a.out / 'unused-malformed.plt'
code = '''[source,output]=System.argv(); {plt,info}=:dialyzer_cplt.plt_and_info_from_file(String.to_charlist(source)); name={:lists,:merge3,3}; true=:ets.member(elem(plt,1),name); true=:ets.member(elem(plt,3),name); true=:ets.insert(elem(plt,3),{name,{:contract,[{:unexpected_type,[]}],[:any,:any,:any],[]}}); :ok=:dialyzer_cplt.to_file(String.to_charlist(output),plt,:dict.new(),info)'''
subprocess.run(['elixir', '-e', code, str(prior), str(mutated)], env=env, check=True, capture_output=True)
results = {}
for backend in ['native', 'rust']:
    for case, library in [('clean', prior), ('unused_malformed', mutated)]:
        command = ([str(otp / 'bin/dialyzer'), '-pa', str(elixir / 'lib/elixir/ebin')]
                   if backend == 'native' else [str(a.candidate)])
        run = subprocess.run(command + ['--no_check_plt', '--plt', str(library), '-Wno_unknown', *map(str, a.beams)],
                             env=env, capture_output=True, timeout=600)
        name = f'{backend}-{case}'
        (a.out / (name + '.stdout')).write_bytes(run.stdout)
        (a.out / (name + '.stderr')).write_bytes(run.stderr)
        results[name] = dict(exit_status=run.returncode, warnings=warnings(run.stdout), stderr=run.stderr.decode())
assert results['native-clean'] == results['native-unused_malformed'], 'Native reached the unused contract'
assert results['native-clean']['exit_status'] in (0, 2)
assert results['rust-clean']['exit_status'] == results['native-clean']['exit_status']
assert results['rust-clean']['warnings'] == results['native-clean']['warnings']
assert inventory() == before
report = dict(native_ignores_unused_contract=True,
              rust_matches_unused_contract=results['rust-unused_malformed']['exit_status'] == results['native-unused_malformed']['exit_status']
                  and results['rust-unused_malformed']['warnings'] == results['native-unused_malformed']['warnings'],
              results=results, inputs_unchanged=True, input_sha256=before,
              mutated_plt_sha256=hashlib.sha256(mutated.read_bytes()).hexdigest())
(a.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({k: v for k, v in report.items() if k not in ('results', 'input_sha256')}))
