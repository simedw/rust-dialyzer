#!/usr/bin/env python3
import hashlib,json,random,subprocess
from pathlib import Path
from form_export_probes.cache_fixture_lib import A,arena
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/warning_collection';OUT.mkdir(parents=True,exist_ok=True)
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
files=[ROOT/p for p in ['src/warning_collection.rs','src/bin/warning_collection_probe.rs','src/core.rs','src/term_order.rs','src/program.rs','src/type_exports.rs','scripts/warning_collection_oracle.exs','scripts/core_export.exs','scripts/form_export_probes/cache_fixture_lib.py','Cargo.toml','Cargo.lock']]+[Path(__file__).resolve()]
hashes={str(p.relative_to(ROOT)):digest(p)for p in files};rng=random.Random(4901);rows=[]
paths=[[ord(c)for c in p]for p in ['a.erl','b.erl','/src/a.erl']]
locations=[1,4,(4,3),(7,8)]
mfas=[(A('m'),A('f'),0),(A('m'),A('g'),1),(A('n'),A('f'),2)]
for index in range(5000):
 contracts=[(m,(rng.choice(paths),rng.choice(locations)))for m in mfas if rng.random()<.7];warnings=[]
 for _ in range(rng.randrange(16)):
  mfa=rng.choice(mfas);file=rng.choice(paths);line=rng.choice(locations);info=(file,line,mfa);kind=rng.randrange(4)
  if kind==0:
   warnings.append((A('warn_contract_range'),info,(A('contract_range'),[[ord(c)for c in 'spec'],*mfa,[[ord(c)for c in 'args']],[ord(c)for c in 'return']])))
  elif kind==1:
   if contracts and rng.random()<.6:
    own,origin=rng.choice(contracts);info=(*origin,own)
   warnings.append((A('warn_contract_types'),info,(A('invalid_contract'),list(mfa))))
  else:warnings.append((A('warn_not_called'),info,(A('unused_fun'),list(mfa))))
  if warnings and rng.random()<.1:warnings.append(warnings[-1])
 rows.append(dict(id=index,arena=arena((warnings,contracts))))
requests=OUT/'requests.jsonl';requests.write_text(''.join(json.dumps(r)+'\n'for r in rows));request_hash=digest(requests)
subprocess.run(['cargo','build','--release','--offline','--bin','warning_collection_probe'],cwd=ROOT,check=True)
native=OUT/'native.jsonl';rust=OUT/'rust.jsonl'
subprocess.run(['elixir','scripts/warning_collection_oracle.exs','native',str(requests),str(native)],cwd=ROOT,check=True)
with requests.open()as inp,rust.open('w')as out:subprocess.run([str(ROOT/'target/release/warning_collection_probe')],cwd=ROOT,stdin=inp,stdout=out,check=True)
for path in [native,rust]:
 records=[json.loads(s)for s in path.read_text().splitlines()]
 if len(records)!=5000 or [r.get('id')for r in records]!=list(range(5000)):raise RuntimeError('collection outcome identities changed')
subprocess.run(['elixir','scripts/warning_collection_oracle.exs','compare',str(native),str(rust)],cwd=ROOT,check=True)
if hashes!={str(p.relative_to(ROOT)):digest(p)for p in files} or digest(requests)!=request_hash:raise RuntimeError('collection proof changed during verification')
source=ROOT/'.cache/upstream/otp/lib/dialyzer/src/dialyzer_succ_typings.erl'
report=dict(commands=5000,matched=5000,complete_contract_warning_postprocessing=True,complete_warning_production=False,implementation_sha256=hashes,inputs_sha256={str(source.relative_to(ROOT)):digest(source),str(requests.relative_to(ROOT)):request_hash},unsupported=['Core warning production','invalid contract and callback collection','arbitrary malformed raw warning boundary'])
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
