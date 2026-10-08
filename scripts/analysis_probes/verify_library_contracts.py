#!/usr/bin/env python3
import hashlib,json,subprocess,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];HERE=Path(__file__).resolve().parent;OUT=ROOT/'.cache/library_contracts';OUT.mkdir(parents=True,exist_ok=True)
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
core=ROOT/'.cache/corpus/otp/otp_small_lzip-8836913f28/core-v6.json';library=ROOT/'.cache/dependencies/base-573d6b6f76baf86026e6ed855a238266a2d36b89919a88ce5211465196227f56.json';plt=Path(json.loads(library.read_text())['source']['path']);binary=ROOT/'target/release/library_contracts_probe'
files=list((ROOT/'src').rglob('*.rs'))+[core,library,plt,ROOT/'Cargo.toml',ROOT/'Cargo.lock',ROOT/'scripts/native_types.exs',HERE/'native_library_contracts.exs',Path(__file__).resolve()];sources={str(p):sha(p) for p in files};binary_sha=sha(binary);start=time.monotonic()
native=OUT/'native.jsonl';actual=OUT/'rust.jsonl';subprocess.run(['elixir',str(HERE/'native_library_contracts.exs'),str(ROOT),str(core),str(library),str(native)],cwd=ROOT,check=True);input_sha=sha(native)
with actual.open('wb') as out:subprocess.run([str(binary),str(core),str(library)],stdout=out,cwd=ROOT,check=True)
def expand(graph):
 terms=graph['terms'];memo={};work=[(graph['root'],False)]
 while work:
  node,ready=work.pop()
  if node in memo:continue
  term=terms[node];kind=term['kind'];children=term.get('items',[])
  if kind=='list':children=children+[term['tail']]
  if kind=='map':children=[x for pair in term['entries'] for x in pair]
  if not ready and children:
   work.append((node,True));work.extend((child,False) for child in reversed(children));continue
  result=dict(term)
  if kind=='tuple':result['items']=[memo[x] for x in term['items']]
  if kind=='list':
   result['items']=[memo[x] for x in term['items']];tail=memo[term['tail']]
   if tail['kind']=='list':result['items']+=tail['items'];tail=tail['tail']
   result['tail']=tail
  if kind=='map':result['entries']=[[memo[k],memo[v]] for k,v in term['entries']]
  memo[node]=result
 return memo[graph['root']]
key=lambda x:(x['kind'],x['mfa']['module'],x['mfa']['function'],x['mfa']['arity'])
n={key(x):x for x in map(json.loads,native.read_text().splitlines())};r={key(x):x for x in map(json.loads,actual.read_text().splitlines())}
for row in r.values():row['record']=expand(row['record'])
failures=[k for k in n if n[k]!=r.get(k)];assert len(n)==len(r)==3314 and not failures,(len(n),len(r),failures[:10])
assert sources=={str(p):sha(p) for p in files};assert sha(native)==input_sha and sha(binary)==binary_sha
report=dict(records=len(n),contracts=sum(k[0]=='contract' for k in n),callbacks=sum(k[0]=='callback' for k in n),source_sha256=sources,binary_sha256=binary_sha,native_sha256=input_sha,output_sha256=sha(actual),elapsed_seconds=time.monotonic()-start)
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print({k:report[k] for k in ['records','contracts','callbacks','elapsed_seconds']})
