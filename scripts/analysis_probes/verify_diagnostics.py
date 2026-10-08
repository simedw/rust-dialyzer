import hashlib,json,os,subprocess,collections
from pathlib import Path
H=Path(__file__).resolve().parent;R=H.parents[1];P=R/'.cache/contract_diagnostics';P.mkdir(parents=True,exist_ok=True);binary=R/'target/release/diagnostics_probe'
files=[Path(x)for x in (R/'target/release/diagnostics_probe.d').read_text().splitlines()[0].split(': ',1)[1].split()]+[H/x for x in ['native_diagnostics.exs','generate_diagnostics.py','verify_diagnostics.py']]+[R/'scripts'/x for x in ['core_export.exs','type_codec.exs']]+[R/'corpus/bifs.json']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes():return {str(p):sha(p)for p in files}
env=os.environ|{'ASDF_ERLANG_VERSION':'28.3','ASDF_ELIXIR_VERSION':'1.19.4-otp-28'}
beams=subprocess.check_output(['elixir','-e','Enum.each([:dialyzer_contracts,:dialyzer_behaviours,:dialyzer_plt,:dialyzer_codeserver,:erl_types,:erl_bif_types],fn m->IO.puts(:code.which(m))end)'],env=env,text=True).splitlines();files.extend(map(Path,beams))
before=hashes();binary_sha=sha(binary)
assert binary.stat().st_mtime_ns >=max(p.stat().st_mtime_ns for p in files if p.suffix=='.rs'),'stale probe'
subprocess.run(['python3',str(H/'generate_diagnostics.py'),str(P)],check=True)
requests=P/'diagnostics-requests.jsonl';request_sha=sha(requests)
subprocess.run(['elixir',str(H/'native_diagnostics.exs'),str(R),str(requests),str(P/'diagnostics-commands.jsonl')],check=True,env=os.environ|{'ASDF_ERLANG_VERSION':'28.3','ASDF_ELIXIR_VERSION':'1.19.4-otp-28'})
commands=P/'diagnostics-commands.jsonl';input_sha=sha(commands);rows=[json.loads(x)for x in commands.read_text().splitlines()]
result=subprocess.check_output([str(binary)],input=commands.read_bytes());(P/'diagnostics-rust.jsonl').write_bytes(result);actual=[json.loads(x)for x in result.splitlines()]
assert len(actual)==len(rows)==3198
failures=[{'index':i,'input':n,'rust':a}for i,(n,a)in enumerate(zip(rows,actual))if a.get('matched')is not True or(n['kind']=='contracts' and a.get('accepted_matched')is not True)]
(P/'diagnostics-failures.json').write_text(json.dumps(failures[:15],indent=2));assert not failures,len(failures)
profile={'dialyzer_contracts':'75CF94EF5951B50537E90EAB8CB718EC64CBEFB61BDCA071B35851ABBD1C27CB','dialyzer_behaviours':'8C8F16CD5B4FC1B1FE82C72011FFEA7CC089E4364316E598F18EF62DD97F8B0E'}
assert all(r['profile']==profile for r in rows)
assert sum(a.get('accepted_matched')is True for a in actual)==1886
categories=collections.Counter();messages=collections.Counter()
for a in actual:
 arena=a['actual'];terms=arena['terms'];root=arena['root']
 while terms[root]['kind']=='list':
  for wid in terms[root]['items']:
   w=terms[wid];categories[terms[w['items'][0]]['value']]+=1
   msg=terms[w['items'][2]];messages[terms[msg['items'][0]]['value']]+=1
  root=terms[root]['tail']
required={'invalid_contract','overlapping_contract','extra_range','missing_range','contract_subtype','contract_supertype','contract_diff','contract_with_opaque','callback_missing','callback_info_missing','callback_not_exported','callback_type_mismatch','callback_arg_type_mismatch','callback_spec_type_mismatch','callback_spec_arg_type_mismatch'}
assert required<=messages.keys(),required-messages.keys()
assert before==hashes(),'sources changed during proof';assert binary_sha==sha(binary)and request_sha==sha(requests)and input_sha==sha(commands)
report={'cases':3198,'matched':3198,'contract_cases':sum(r['kind']=='contracts'for r in rows),'callback_cases':sum(r['kind']=='callbacks'for r in rows),'accepted_contract_cases':sum(a.get('accepted_matched')is True for a in actual),'raw_warnings':sum(a['warnings']for a in actual),'categories':dict(categories),'messages':dict(messages),'original_beam_sha256':profile,'source_sha256':before,'binary_sha256':binary_sha,'request_sha256':request_sha,'input_sha256':input_sha,'checked':['actual original get_invalid_contract_warnings/3, check_contracts/3 and check_callbacks/5','complete raw warning categories, origins, payloads, presentation strings and collection order','single/overloaded/constrained contracts and final Rust signature inputs','BIF signature override and raise/3 exception','original record tables and record abstraction','remote suppression and local alias no-remote resolution','missing/exported/optional callbacks, declaration and function origins','behaviour/behavior spelling and repeated first declaration locations','multiple argument mismatches and actual PLT callback list order','module ordering and flat/HAMT contract map traversal'],'boundary':['collector consumes ordered entries; independent production map order belongs to native_map_order','final signatures are controlled Rust-owned inputs, no new inference path','prepared contract summary and raw capture are trusted coherent metadata'],'full_analysis':False}
(P/'diagnostics-verification.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k]for k in ['cases','matched','contract_cases','callback_cases','raw_warnings']}))
