#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
from proof_inputs import directory_entries
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/consistency';OUT.mkdir(parents=True,exist_ok=True)
inventory=ROOT/'corpus/inventory.json';registry=json.loads(inventory.read_text());suites=registry['otp_generated_suites'];assert len(suites)==22
files=[inventory,ROOT/'src/bin/corpus_registry_probe.rs',ROOT/'scripts/consistency_oracle.exs',ROOT/'.cache/upstream/otp/lib/dialyzer/test/dialyzer_common.erl',ROOT/'.cache/upstream/otp/lib/dialyzer/test/file_utils.erl',ROOT/'Cargo.toml',ROOT/'Cargo.lock',Path(__file__).resolve(),ROOT/'scripts/proof_inputs.py']
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):digest(p)for p in files}
before=hashes();commands=[];directories={};names={r['id']:r for r in registry['semantic_cases']if r['kind']=='otp'}
for suite in suites:
 cases=[names[id]for id in suite['cases']];assert cases
 directory=ROOT/'.cache/upstream'/cases[0]['suite_dir']
 directories[str(directory/'src')]=directory_entries(directory/'src')
 declared=[r['case']for r in cases]+[suite['suite']+'_SUITE_consistency']
 commands += [dict(id=suite['suite']+'/complete',suite_dir=str(directory),declared=declared),dict(id=suite['suite']+'/omitted',suite_dir=str(directory),declared=declared[1:])]
request=OUT/'commands.jsonl';request.write_text(''.join(json.dumps(c)+'\n'for c in commands));input_hash=digest(request)
subprocess.run(['cargo','build','--release','--offline','--bin','corpus_registry_probe'],cwd=ROOT,check=True)
subprocess.run(['elixir',str(ROOT/'scripts/consistency_oracle.exs'),str(ROOT),str(request),str(OUT/'native.jsonl')],cwd=ROOT,check=True)
result=subprocess.check_output([str(ROOT/'target/release/corpus_registry_probe')],input=request.read_bytes());(OUT/'rust.jsonl').write_bytes(result)
native=[json.loads(s)for s in (OUT/'native.jsonl').read_text().splitlines()];rust=[json.loads(s)for s in result.splitlines()]
assert len(native)==len(rust)==44 and native==rust
for i,row in enumerate(native):assert (not row['missing']) if i%2==0 else len(row['missing'])==1
assert before==hashes() and input_hash==digest(request)
assert all(directory_entries(path)==entries for path,entries in directories.items())
report=dict(complete_generated_consistency=True,suites=22,outcomes=44,matched=44,registered_otp_semantic_cases=len(names),implementation_sha256=before,inputs_sha256={str(request):input_hash},directory_inputs=directories,verified_suites=[s['suite']for s in suites],scope='complete source registry and omitted-case detection; analyzer semantics verified separately')
(OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k]for k in ['suites','outcomes','matched','registered_otp_semantic_cases']}))
