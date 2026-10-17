import json,random,subprocess
from pathlib import Path
from probe_config import configuration, read_commands
WORKSPACE=Path(__file__).resolve().parents[2];CONFIG=configuration(WORKSPACE);root=CONFIG.out;r=random.Random(28309)
preds='any none none_or_unit unit impossible atom boolean binary bitstring function identifier pid port reference number float integer byte char cons nil list maybe_improper_list string map tuple non_negative_integer variable singleton opaque'.split()
ctors='any none unit atom binary nonempty_binary bitstring nonempty_bitstring boolean byte char function identifier pid port reference number float integer positive_integer non_negative_integer negative_integer fixnum positive_fixnum non_negative_fixnum cons nil list nonempty_list string nonempty_string maybe_improper_list map tuple arity mfa module node iolist iodata bitstring_list timeout'.split()
values={}
for file in ['nominal_commands.jsonl','raw_commands.jsonl','subst_commands.jsonl']:
 for line in read_commands(root/file, CONFIG):
  c=json.loads(line);a=c['a'];values[json.dumps(a,sort_keys=True)]=a
pool=list(values.values());types=r.sample(pool,min(2500,len(pool)))
# Scalar and range boundaries covered even if not chosen from random nesting.
k=lambda kind,**kw:dict(kind=kind,**kw)
types += [k('atoms',values=v)for v in [['false'],['true'],['false','true'],['a']]]+[k('range',**dict(zip(['from','to'],pair)))for pair in [('0','255'),('0','256'),('0','1114111'),('0','1114112'),('neg_inf','0'),('1','pos_inf')]]
commands=[dict(op='predicate',name=name,a=a)for name in preds for a in types]
commands += [dict(op='number_literal',a=k('none'),value=str(n))for n in [-1,0,1,-(1<<180),1<<180]]
commands += [dict(op='builtin',name=name,a=k('none'))for name in ctors]
commands += [dict(op='atom_literal',name=name,a=a)for name in ['a','false','true','missing']for a in types[:500]]
commands += [dict(op='same_opaque',a=a,b=b)for a in types[:80]for b in types[:80]]
getters='atom_values number_values number_min number_max bitstring_unit bitstring_base function_args function_arity function_range cons_head cons_tail list_elements list_termination tuple_args tuple_size tuple_sizes tuple_subtypes map_entries map_default_key map_default_value variable_name nominal_module to_type_list'.split()
commands += [dict(op='getter',name=name,a=a)for name in getters for a in types]
commands += [dict(op=op,a=a)for op in ['lift_list','is_native_type']for a in types]
for _ in range(2500):
 ts=r.sample(types,r.randint(0,5));a=r.choice(types)
 commands += [dict(op=op,a=a,types=ts)for op in ['supremum','infimum','subtract_types','bitstr_concat_types','any_none','any_none_or_unit']]
commands += [dict(op='widen_to_number',a=a)for a in types]
commands += [dict(op='native_order',a=r.choice(types),b=r.choice(types))for _ in range(15000)]
p=root/'api_commands.jsonl';out=root/'api_results.jsonl';p.write_text(''.join(json.dumps(c)+'\n'for c in commands))
with p.open('rb')as src,out.open('wb')as dest:subprocess.run([str(CONFIG.binary)],stdin=src,stdout=dest,check=True)
subprocess.run(['elixir',str(WORKSPACE/'scripts/type_oracle.exs'),str(p),str(out)],check=True)
