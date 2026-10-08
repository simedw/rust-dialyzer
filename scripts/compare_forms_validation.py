"""Compare actual native references; consumes every row and rejects incomplete cases."""
import argparse,json,subprocess
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument('--probe',required=True,type=Path)
parser.add_argument('--operation',choices=['validation','compare','local-contracts'],required=True)
parser.add_argument('--requests',required=True,type=Path)
parser.add_argument('--references',required=True,type=Path)
parser.add_argument('--output',required=True,type=Path)
args=parser.parse_args()
requests=[json.loads(x) for x in args.requests.read_text().splitlines()]
references=[json.loads(x) for x in args.references.read_text().splitlines()]
if len(requests)!=len(references):raise SystemExit('native reference count does not match request count')
rows=[dict(core=req['core'],reference=ref,**({'environment':req['environment']} if 'environment' in req else {})) for req,ref in zip(requests,references)]
with args.output.open('w') as output:
 subprocess.run([str(args.probe),'--'+args.operation],input=''.join(json.dumps(row)+'\n' for row in rows),stdout=output,text=True,check=True)
results=[json.loads(x) for x in args.output.read_text().splitlines()]
if len(results)!=len(rows):raise SystemExit('Rust result count does not match request count')
keys={'validation':['compared','missing_success','native_errors','matched','failed'],'compare':['roots','matched','native_errors','missing_events','failed'],'local-contracts':['contracts','passed','failed','native_errors']}[args.operation]
summary={key:sum(row[key] for row in results) for key in keys};summary['cases']=len(results)
print(json.dumps(summary,sort_keys=True))
args.output.with_suffix('.summary.json').write_text(json.dumps(summary,sort_keys=True)+'\n')
if summary['failed']:raise SystemExit(1)
