import json
from cache_fixture_lib import A,arena,table,remote,builtin,integer
rows=[]
field=lambda name,body:builtin('field_type',[(A('atom'),0,A(name)),body])
record=lambda fields:builtin('record',[(A('atom'),0,A('r'))]+fields)
for mode in ['resolve','check_fields']:
 for label,fs in [('unknown',[field('missing',remote('absent','x'))]),('duplicate',[field('f',remote('absent','x')),field('f',remote('absent','y'))]),('later_event',[field('missing',integer(1)),field('f',remote('absent','later'))]),('good',[field('f',integer(1))]),('badtype',[field('f',(A('atom'),0,A('bad')))])]:
  rows.append(dict(label=mode+'_'+label,snapshots=[arena({A('m'):table('m',1)})],exports=arena([]),steps=[dict(snapshot=0,form=arena(record(fs)),mode=mode,site=dict(kind='spec',module='m',name='probe',arity=0))]))
open('record_context_cases.jsonl','w').writelines(json.dumps(c)+'\n' for c in rows)
