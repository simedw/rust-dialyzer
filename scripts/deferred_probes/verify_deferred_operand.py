#!/usr/bin/env python3
"""Replay native gates; classify unsupported and timeout outcomes explicitly."""
import argparse,hashlib,json,os,subprocess
from pathlib import Path
p=Path(__file__).resolve().parent
args=argparse.ArgumentParser()
args.add_argument('--cache-probe',required=True)
args.add_argument('--operand-probe',required=True)
a=args.parse_args()
cache=str(Path(a.cache_probe).resolve()); operand=str(Path(a.operand_probe).resolve())
os.chdir(p)
def run(command,inp=None,out=None):
 with open(inp,'rb') if inp else open(os.devnull,'rb') as i,open(out,'wb') if out else open(os.devnull,'wb') as o:
  subprocess.run(command,stdin=i,stdout=o,check=True,timeout=300)
def report(file,counts):
 r=json.loads(Path(file).read_text())
 for k,v in counts.items():
  if r.get(k)!=v:raise AssertionError((file,k,r.get(k),v))
 return r
run(['python3','cache_cases.py','--probe',cache],out='cache_gate.stdout')
report('cache_results.jsonl.comparison.json',dict(matched=11000,failed=0,outcomes=11000))
run(['python3','context_status_cases.py','--probe',cache],out='site_gate.stdout')
report('context_status_results.jsonl.comparison.json',dict(matched=60,failed=0,outcomes=60))
for generator,inp,reference,result,count,reference_script,oracle in [
 ('native_context_boundary_cases.py','native_context_boundary_cases.jsonl','boundary_reference.jsonl','boundary_verified.jsonl',16,'native_context_boundary_reference.exs','boundary_full_oracle.exs'),
 ('record_context_cases.py','record_context_cases.jsonl','record_context_reference.jsonl','record_context_results.jsonl',10,'native_context_boundary_reference.exs','boundary_full_oracle.exs'),
 ('deferred_context_cases.py','deferred_context_cases.jsonl','deferred_context_reference.jsonl','deferred_context_results.jsonl',572,'deferred_context_reference.exs','deferred_context_oracle.exs')]:
 run(['python3',generator])
 if count==572:run(['elixir',reference_script,inp,reference])
 else:run(['elixir',reference_script,inp],out=reference)
 run([cache],inp=inp,out=result)
 run(['elixir',oracle,reference,result],out=result+'.stdout')
 report(result+'.full_comparison.json',dict(cases=count,failures=0))
run(['python3','native_operand_cases.py'])
run([operand],inp='native_operand_commands.jsonl',out='native_operand_results.jsonl')
run(['elixir','native_operand_oracle.exs','native_operand_commands.jsonl','native_operand_results.jsonl'],out='native_operand_gate.stdout')
r=report('native_operand_results.jsonl.comparison.json',dict(commands=18380,matches=16716,mismatches=0,unsupported=1662,full_native_operand_parity=False))
if [x['index'] for x in r['oracle_timeouts']]!=[15852,15853]:raise AssertionError(r['oracle_timeouts'])
files=list((p/'src').rglob('*.rs'))+list(p.glob('*.py'))+list(p.glob('*.exs'))+list(p.glob('*commands.jsonl'))+[p/'deferred_context_cases.jsonl',p/'native_context_boundary_cases.jsonl',p/'record_context_cases.jsonl']
summary={'deferred_context_matches':572,'native_operand':r,'source_input_sha256':{str(f.relative_to(p)):hashlib.sha256(f.read_bytes()).hexdigest()for f in sorted(set(files))},'timeout_scope':'decode + native call + comparison;1000ms per task','full_native_operand_parity':False}
(p/'deferred_operand_gate.json').write_text(json.dumps(summary,indent=2)+'\n')
print('572 exact deferred outcomes;16716 raw matches;1662 unsupported;2 oracle timeouts;0 mismatches')
