"""Run the existing analyzer on separately captured OTP Core oracle rows."""
import argparse
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('repo', type=Path)
parser.add_argument('--case', action='append', default=[])
args = parser.parse_args()
base = Path(__file__).resolve().parent
oracle = base / 'generated/oracle'
rows = [json.loads(line) for line in (oracle/'oracle-rows.jsonl').read_text().splitlines()]
if args.case:
    rows = [row for row in rows if row['id'] in args.case]
selected = oracle/'baseline-selected.jsonl'
selected.write_text(''.join(json.dumps(row)+'\n' for row in rows))
with selected.open() as incoming, (oracle/'baseline-results.jsonl').open('w') as outgoing:
    subprocess.run([str(args.repo.resolve()/'target/debug/compiled_analysis_probe'),
                    str(oracle/'prior-library.json'), str(oracle/'closure-profile.json')],
                   stdin=incoming, stdout=outgoing, check=True)
actual = [json.loads(line) for line in (oracle/'baseline-results.jsonl').read_text().splitlines()]
assert len(actual) == len(rows)
checks = [dict(id=row['id'], expected_warnings=row['warning_count'], exact_warnings=result.get('public_warnings_match') is True, result=result) for row,result in zip(rows,actual)]
report = dict(cases=len(rows), exact_warning_matches=sum(c['exact_warnings'] for c in checks), checks=checks,
              scope='OTP Core baseline only; not evidence of native Elixir frontend correctness')
(oracle/'baseline-report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k != 'checks'}))
raise SystemExit(0 if all(c['exact_warnings'] for c in checks) else 1)
