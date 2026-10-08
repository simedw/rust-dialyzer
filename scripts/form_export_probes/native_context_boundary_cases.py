import json
from cache_fixture_lib import A,arena,table,user,remote,builtin,integer
rows=[]
def put(label,modules,exports,form,site='spec'):
 rows.append(dict(label=label,snapshots=[arena(modules)],exports=arena(exports),steps=[dict(snapshot=0,form=arena(form),mode='resolve' if site=='spec' else 'check_remote',site=dict(kind=site,module='m',name='probe',arity=0))]))
base=table('m',1)
# Native lazily ignores unrelated keys and body metadata.
for bad in [A('malformed'),(A('type'),A('unused'),0), (A('record'),A('unused'))]:
 t=base|{bad:A('bad_metadata')};put('unused_bad_'+repr(bad),{A('m'):t},[],integer(1))
# Check verifies existence and arguments, but does not inspect alias body/params.
for body,params in [(A('bad_form'),[]),(integer(1),[A('extra')]),((A('type'),0,A('unknown_builtin'),[]),[])]:
 t=base|{(A('type'),A('skip'),0):((A('m'),([102],1),body,params),A('any'))}
 put('check_skipped_body_'+repr(body),{A('m'):t},[],user('skip'),'check')
# Lookup module identity and definition identity are independent.
t=base|{(A('opaque'),A('foreign'),0):((A('other'),([102],1),integer(4),[]),A('any'))}
put('foreign_defined_opaque',{A('m'):t},[],user('foreign'))
# Exact error reasons and native throw iolist shapes.
put('unknown_local_module',{},[],user('missing'))
put('unknown_local_type',{A('m'):{}},[],user('missing'))
put('stale_remote_export',{A('m'):{}},[(A('m'),A('missing'),0)],remote('m','missing'))
put('unknown_record',{A('m'):{}},[],builtin('record',[(A('atom'),0,A('missing'))]))
put('bad_range_expression',{A('m'):{}},[],builtin('range',[(A('atom'),0,A('a')),integer(1)]))
# Events retain origin metadata and ordering, even though result is any.
put('missing_remote',{},[],remote('absent','missing'))
put('unexported_remote',{A('m'):base},[],remote('m','a'))
put('duplicate_events',{},[],builtin('union',[remote('absent','missing'),remote('absent','missing')]))
put('line_column_event',{},[],(A('remote_type'),(19,7),[(A('atom'),0,A('absent')),(A('atom'),0,A('missing')),[]]))
open('native_context_boundary_cases.jsonl','w').writelines(json.dumps(c)+'\n' for c in rows)
