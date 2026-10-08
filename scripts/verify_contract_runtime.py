import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
SOURCE=ROOT/'.cache/dependencies/otp-base.json'
PINNED_SOURCE=SOURCE
OUT=ROOT/'.cache/contracts'
OUT.mkdir(parents=True,exist_ok=True)
proof_files=sorted((ROOT/'src').rglob('*.rs'))+[ROOT/'Cargo.toml',ROOT/'Cargo.lock',Path(__file__).resolve(),ROOT/'scripts/contracts_oracle.exs',ROOT/'scripts/augment_contract_fixture.exs',ROOT/'scripts/native_types.exs']
def implementation_hashes():
 return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in proof_files}
initial_hashes=implementation_hashes()
source_hash=hashlib.sha256(PINNED_SOURCE.read_bytes()).hexdigest()
subprocess.run(['cargo','build','--release','--offline','--bin','forms_probe'],cwd=ROOT,check=True)
SOURCE=OUT/'augmented_contracts.json'
subprocess.run(['elixir','scripts/augment_contract_fixture.exs',PINNED_SOURCE,SOURCE],cwd=ROOT,check=True)
contracts=json.loads(SOURCE.read_text())['contracts']
ctor=lambda name,*args:dict(constructor=name,arguments=list(args))
atom=lambda value:dict(atom=value)
I=ctor('t_integer');A=ctor('t_atom',atom('sample'))
pool=[ctor('t_integer',dict(integer='1')),A,ctor('t_list',I),ctor('t_nominal',dict(tuple=[atom('sample'),atom('integer'),dict(integer='0'),atom('transparent')]),I),ctor('t_nominal',dict(tuple=[atom('sample'),atom('integer'),dict(integer='0'),atom('opaque')]),I)]
requests=[]
for ci,item in enumerate(contracts):
 c={key:item['contract'][key] for key in ['alternatives','arguments']};arity=item['mfa']['arity']
 requests.append(dict(id=str(len(requests)),contract_index=ci))
 for name in ['t_any','t_none','t_unit']:requests.append(dict(id=str(len(requests)),contract_index=ci,arguments=[ctor(name)]*arity))
 # baseline arguments are native types; transfer descriptors accept ETF only, so universal substituted grid supplies actual arguments here.
 for position in range(arity):
  for value in pool:
   args=[ctor('t_any')]*arity;args[position]=value;requests.append(dict(id=str(len(requests)),contract_index=ci,arguments=args))
path=OUT/'runtime_contract_requests.jsonl';path.write_text(''.join(json.dumps(x)+'\n' for x in requests))
with (OUT/'runtime_contract_native_arena.json').open('w') as out:subprocess.run(['elixir','scripts/contracts_oracle.exs',SOURCE,path],stdout=out,cwd=ROOT,check=True)
with (OUT/'runtime_contract_results.jsonl').open('w') as out:subprocess.run([ROOT/'target/release/forms_probe','--runtime-arena',SOURCE,path,OUT/'runtime_contract_native_arena.json'],stdout=out,check=True)
results=[json.loads(x) for x in (OUT/'runtime_contract_results.jsonl').read_text().splitlines()]
if len(results)!=len(requests):raise RuntimeError("runtime contract result denominator changed")
if [r["id"] for r in results]!=[r["id"] for r in requests]:raise RuntimeError("runtime contract result identities changed")
failures=[dict(request=requests[int(r['id'])],response=r) for r in results if not r['parity'] or not r['domain_parity']]
(OUT/'runtime_contract_failures.json').write_text(json.dumps(failures))
summary=dict(contracts=len(contracts),requests=len(requests),matched=len(results)-len(failures),failed=len(failures),alternatives=sum(len(x['contract']['alternatives']) for x in contracts),constrained=sum(bool(a['constraints']) for x in contracts for a in x['contract']['alternatives']))
(OUT/'runtime_contract_summary.json').write_text(json.dumps(summary));print(summary)

if implementation_hashes()!=initial_hashes:raise RuntimeError('runtime contract implementation changed during verification')
if hashlib.sha256(PINNED_SOURCE.read_bytes()).hexdigest()!=source_hash:raise RuntimeError('runtime contract inputs changed during verification')
summary.update(complete_runtime_contracts=not failures,implementation_sha256=initial_hashes,inputs_sha256={str(PINNED_SOURCE.relative_to(ROOT)):source_hash})
(OUT/'runtime_contract_summary.json').write_text(json.dumps(summary,indent=2)+'\n')
if failures:raise SystemExit(1)
