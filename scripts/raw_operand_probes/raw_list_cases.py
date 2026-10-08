import json,random
from raw_scalar_cases import A,arena
c=lambda tag,e,q=A('unknown'):(A('c'),A(tag),e,q)
scalar=[A('any'),A('none'),A('unit'),A('bad'),c('atom',[A('a')]),c('atom',[A('b')]),c('atom',A('bad')),c('nil',[]),c('nil',A('bad'),A('weird')),c('var',A('X'),A('weird'))]
values=[c('list',(a,b),q)for a in scalar[:7]for b in scalar[:7]+[c('nil',[])]for q in [A('unknown'),A('nonempty'),A('weird')]]
rows=[dict(op='inf',left=arena(a),right=arena(b))for a in values for b in values]
for a in values:
 for nil in [c('nil',[]),c('nil',A('bad'),A('weird'))]:
  rows += [dict(op='inf',left=arena(a),right=arena(nil)),dict(op='inf',left=arena(nil),right=arena(a))]
open('raw_list_commands.jsonl','w').writelines(json.dumps(x)+'\n'for x in rows)
print(len(rows))
