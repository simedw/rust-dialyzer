import json,pathlib
pool=[{'kind':k} for k in ['any','none','unit','opaque','atom','integer','float','number','binary','nil','map_any','tuple_any']]
pool += [{'kind':'atoms','values':v} for v in [['true'],['false'],['true','false'],['ok'],['false','ok']]]
pool += [{'kind':'integers','values':['-3','0','9']},{'kind':'range','from':'-5','to':'7'},{'kind':'bitstring','unit':'3','base':'2'},{'kind':'var','id':{'atom':'A'}}]
pool += [{'kind':'list','elements':{'kind':'integer'},'tail':tail,'nonempty':ne} for tail in [{'kind':'nil'},{'kind':'atom'},{'kind':'any'}] for ne in [False,True]]
pool += [{'kind':'tuple','elements':[{'kind':'atoms','values':['tag']},{'kind':'integer'}]},{'kind':'function','domain':[{'kind':'integer'}],'range':{'kind':'atom'}}]
for base in list(pool):
 if base['kind'] not in ['opaque','var','unit','none']:
  for opacity in ['transparent','opaque']:
   pool.append({'kind':'nominal','name':{'module':'guard_fixture','name':'wrapped','arity':0,'opacity':opacity},'inner':base})
rows=[]
for eval in ['positive','negative','unknown']:
 for actual in pool:
  rows.append({'op':'eval_inf','eval':eval,'actual':actual})
  for expected in pool:rows.append({'op':'bind_test','eval':eval,'actual':actual,'expected':expected})
for name in ['is_atom','is_boolean','is_binary','is_bitstring','is_float','is_function','is_integer','is_list','is_map','is_number','is_pid','is_port','is_reference','is_tuple','unknown']:
 for arity in [0,1,2,3]:rows.append({'op':'test_domain','name':name,'arity':arity})
for i,row in enumerate(rows):row['id']=i
pathlib.Path('data/guard_ops_cases.jsonl').write_text(''.join(json.dumps(r,separators=(',',':'))+'\n' for r in rows))
print(json.dumps({'cases':len(rows),'type_pool':len(pool)}))
