#!/usr/bin/env python3
"""Strict actual-private-helper comparisons; graph facts are proof inputs only."""
import argparse,hashlib,json,pathlib

def read(path):return [json.loads(x)for x in pathlib.Path(path).read_text().splitlines()]
def compare(native,rust,count):
 a,b=read(native),read(rust)
 if len(a)!=count or len(b)!=count:raise ValueError(f'expected {count} rows: {len(a)}/{len(b)}')
 for index,(x,y)in enumerate(zip(a,b)):
  if x!=y:raise ValueError(f'mismatch row {index}, id {x.get("id")}: {[k for k in x if x[k]!=y.get(k)]}')
 return a

def main():
 p=argparse.ArgumentParser();p.add_argument('--data',default='data');p.add_argument('--make-inputs',action='store_true');args=p.parse_args();d=pathlib.Path(args.data)
 if args.make_inputs:
  cases,native=read(d/'function_cases.jsonl'),read(d/'function_native.jsonl')
  if len(cases)!=3000 or len(native)!=3000:raise ValueError('partial state proof')
  with(d/'function_inputs.jsonl').open('w')as f:
   for a,b in zip(cases,native):
    if a['id']!=b['id']:raise ValueError('case ids differ')
    a['caller_facts']=b['caller_facts'];f.write(json.dumps(a)+'\n')
  return
 atoms=compare(d/'atom_native.jsonl',d/'atom_rust.jsonl',3000)
 order=compare(d/'order_native.jsonl',d/'order_rust.jsonl',3000)
 states=compare(d/'function_native.jsonl',d/'function_rust.jsonl',3000)
 corpus=compare(d/'corpus_function_native.jsonl',d/'corpus_function_rust.jsonl',327)
 modules=sum(len(r['modules'])for r in corpus);functions=sum(len(m['initial']['entries'])-1 for r in corpus for m in r['modules'])
 if modules!=571 or functions!=13748:raise ValueError('corpus scope differs')
 report={'dict_atom_cases':len(atoms),'dict_worklist_cases':len(order),'state_cases':len(states),'state_transitions':sum(len(r['steps'])for r in states),'incoming_local_edges':sum(len(v)for r in states for v in r['caller_facts'].values()),'corpus_cases':len(corpus),'modules':modules,'functions':functions,'mismatches':0,'hashes':{p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in d.glob('*native.jsonl')}}
 print(json.dumps(report,sort_keys=True));(d/'report.json').write_text(json.dumps(report,indent=2)+'\n')
if __name__=='__main__':main()
