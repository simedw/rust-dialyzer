import json
from cache_fixture_lib import A,arena,table,user,remote,builtin,integer
rows=[]
def put(label,table0,table1,steps):
 rows.append(dict(label=label,snapshots=[arena({A('m'):table0}),arena({A('m'):table1})],exports=arena([(A('m'),A('a'),0)]),steps=steps))
step=lambda form,snapshot=0,mode='resolve',**kw:dict(snapshot=snapshot,form=arena(form),mode=mode,site=dict(kind='spec',module='m',name='probe',arity=0),**kw)
for bad in [A('bad_body'),builtin('unknown_builtin'),builtin('tuple',[A('bad_child')])]:
 t=table('m',1);u=table('m',2);u[(A('type'),A('a'),0)]=((A('m'),([102],1),bad,[]),A('any'))
 put('warm_body_'+repr(bad),t,u,[step(user('a')),step(user('a'),1,clear_module_cache=True),step(user('a'),1,fresh=True)])
for bad in [A('bad_params'),[A('extra')],[(A('bad'),A('variable'))]]:
 t=table('m',1);u=table('m',2);u[(A('type'),A('a'),0)]=((A('m'),([102],1),integer(9),bad),A('any'))
 put('warm_params_'+repr(bad),t,u,[step(user('a')),step(user('a'),1,clear_module_cache=True),step(user('a'),1,fresh=True)])
for levels in [15,16,17,20]:
 body=A('uninspectable_leaf')
 for _ in range(levels):body=builtin('list',[body])
 put('depth_'+str(levels),{}, {},[step(body)])
for root in [(A('ann_type'),0,[A('ignored_variable'),integer(7)]),remote('absent','x',[A('ignored_argument')])]:
 put('skipped_child_'+repr(root),{},{},[step(root)])
field=lambda name,body:builtin('field_type',[(A('atom'),0,A(name)),body])
record=lambda fields:builtin('record',[(A('atom'),0,A('r'))]+fields)
for fields in [[A('malformed_unused')],A('not_a_list'),[(A('f'),A('bad_ast'),A('bad_cached'))],[(A('f'),integer(1),(A('c'),A('number'),(A('int_set'),[1]),A('integer'))),(A('unused'),A('bad_ast'),A('bad_cached'))]]:
 t=table('m',1);t[(A('record'),A('r'))]=(([],1),[(1,fields)])
 for mode in ['check_fields','resolve']:
  for overrides in [[],[field('f',remote('absent','effect'))],[field('f',integer(1))]]:
   put('lazy_record_'+mode+'_'+repr(fields)+'_'+repr(overrides),t,t,[step(record(overrides),mode=mode)])
for name in ['unknown_builtin','integer','tuple','map','binary','range']:
 for children in [[],[integer(1)],[A('bad_child')], [A('bad_child'),A('bad_child')]]:
  put('check_generic_'+name+'_'+repr(children),{}, {},[step(builtin(name,children),mode='check_fields')])
import random
r=random.Random(9913)
for i in range(500):
 body=A('uninspectable_leaf')
 for _ in range(r.randrange(1,25)):
  body=r.choice([lambda b:builtin('list',[b]),lambda b:builtin('tuple',[b]),lambda b:(A('ann_type'),0,[A('ignored_var'),b])])(body)
 put('seeded_depth_'+str(i),{},{},[step(body)])
open('deferred_context_cases.jsonl','w').writelines(json.dumps(c)+'\n'for c in rows)
