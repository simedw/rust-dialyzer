#!/usr/bin/env python3
import argparse,subprocess,os,json,hashlib
from pathlib import Path
p=Path(__file__).resolve().parent
parser=argparse.ArgumentParser();parser.add_argument('--probe',required=True);args=parser.parse_args();probe=str(Path(args.probe).resolve());os.chdir(p)
reports={}
for name,total,expected in [('native_operand',18380,dict(matches=16716,unsupported=1662)),('raw_scalar',14364,dict(matches=14364,unsupported=0)),('raw_structural',46356,dict(matches=46356,unsupported=0)),('raw_list',28896,dict(matches=28896,unsupported=0))]:
 subprocess.run(['python3',name+'_cases.py'],check=True,timeout=30)
 with open(name+'_commands.jsonl','rb')as i,open(name+'_results.jsonl','wb')as o:subprocess.run([probe],stdin=i,stdout=o,check=True,timeout=120)
 with open(name+'_gate.stdout','wb')as o:subprocess.run(['elixir','native_operand_oracle.exs',name+'_commands.jsonl',name+'_results.jsonl'],stdout=o,check=True,timeout=120)
 r=json.load(open(name+'_results.jsonl.comparison.json'))
 for key,value in dict(commands=total,mismatches=0,**expected).items():assert r[key]==value,(name,key,r[key],value)
 if name=='native_operand':assert [t['index']for t in r['oracle_timeouts']]==[15852,15853] and not r['full_native_operand_parity'],r
 else:assert not r['oracle_timeouts'] and r['full_native_operand_parity'],r
 reports[name]=r
files=list((p/'src').rglob('*.rs'))+list(p.glob('*.py'))+list(p.glob('*.exs'))+list(p.glob('*commands.jsonl'))
reports['source_input_sha256']={str(f.relative_to(p)):hashlib.sha256(f.read_bytes()).hexdigest()for f in sorted(files)}
reports['full_native_operand_parity']=False
(p/'raw_continuation_gate.json').write_text(json.dumps(reports,indent=2)+'\n')
print('16716 original matches;1662 Unsupported;2timeouts;89616 new exact scalar/function/product/list outcomes;0mismatches')
