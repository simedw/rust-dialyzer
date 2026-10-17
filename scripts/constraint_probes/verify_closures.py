import json,subprocess,os
from pathlib import Path
from config import configure
config=configure();root=Path(__file__).resolve().parents[2];h=config.out;h.mkdir(parents=True,exist_ok=True)
env=config.env
subprocess.run(['elixir',str(root/'scripts/constraint_probes/closure_metadata.exs'),str(root),str(h/'closure-metadata.json')],env=env,check=True)
d=json.loads((h/'closure-metadata.json').read_text());pairs=d['pairs'];assert len(pairs)==6561 and d['exp_t_only_patch'] is True
profile={k:d[k]for k in ['module','original_sha256','fun_table']}
local=[f for role in d['roles']for f in role['closures']]
commands=[{'closure_profile':profile,'site':f['name'],'environment':f['env_terms'],'expected':f}for f in local]
inputs=''.join(json.dumps(c)+'\n'for c in commands).encode();factory=[json.loads(x)for x in subprocess.check_output([str(config.binary)],input=inputs).splitlines()]
assert len(factory)==len(commands)and all(x=={'order':0}for x in factory),'native code profile instantiation mismatch'
(h/'closure-code-profile.json').write_text(json.dumps(profile,indent=2))
commands=[{'closure_compare':{'a':p['a'],'b':p['b']}}for p in pairs]
input_=''.join(json.dumps(c)+'\n'for c in commands).encode();result=subprocess.check_output([str(config.binary)],input=input_);rust=[json.loads(x)for x in result.splitlines()];assert len(rust)==len(pairs),'incomplete Rust output'
diff=[{'index':i,'rust':r,'native':p['native_order']}for i,(p,r)in enumerate(zip(pairs,rust))if r!={'order':p['native_order']}]
(h/'closure-verification.json').write_text(json.dumps({'pairs':len(pairs),'passed':len(pairs)-len(diff),'differences':diff,'original_beam_sha256':d['original_sha256'],'factory_cases':len(factory),'code_sites':len(d['fun_table']),'expected_code_sites':d['fun_count'],'expected_factory_cases':len(local),'otp':d['otp']},indent=2));print(len(pairs),len(diff))
