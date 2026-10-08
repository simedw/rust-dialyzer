import copy,itertools,json,random
from pathlib import Path
root=Path(__file__).resolve().parents[1]; directory=root/'.cache/warning_policy'; out=directory/'synthetic';out.mkdir(parents=True,exist_ok=True)
base=json.loads((root/'.cache/flow/structural/core.json').read_text());opts=[o['name']for o in json.loads((root/'corpus/warnings.json').read_text())['options']]
fa=('capture',0); cases=[]
for pair in itertools.product(opts+['nowarn_function'],repeat=2):cases.append([('dialyzer',[(list(pair),fa)])])
for pair in itertools.product(['warn_unused_function','nowarn_unused_function'],repeat=2):
 for option in opts+['nowarn_function']:cases.append([('compile',list(pair)),('dialyzer',[(option,fa)])])
for option in opts:cases.extend([[('dialyzer',[option]),('dialyzer',[('unknown' if option=='no_unknown' else 'no_unknown',fa)])],[('dialyzer',[(option,[fa,('uint',1)])])]])
cases.extend([[('dialyzer',[('nowarn_function',('missing',0))])],[('dialyzer',[('no_match',('capture',-1))])],[('dialyzer',[('invalid_option',fa)])],[('dialyzer',['invalid_option'])]])
for i,attrs in enumerate(cases):
 j=copy.deepcopy(base);m=j['modules'][0];terms=m['terms'];nodes=m['nodes'];nil=next(n for n,t in enumerate(terms)if t['kind']=='nil')
 def term(v):
  if isinstance(v,str):t={'kind':'atom','value':v}
  elif isinstance(v,int):t={'kind':'integer','value':str(v)}
  elif isinstance(v,tuple):t={'kind':'tuple','items':[term(a)for a in v]}
  elif isinstance(v,list):t={'kind':'list','items':[term(a)for a in v],'tail':nil}if v else {'kind':'nil'}
  else:raise ValueError(v)
  terms.append(t);return len(terms)-1
 def lit(v):
  t=term(v);nodes.append({'kind':'literal','annotations':nil,'groups':[],'value':t});return len(nodes)-1
 attr=[]
 for k,v in attrs:attr.extend([lit(k),lit(v)])
 mod=copy.deepcopy(nodes[m['core']]);mod['groups'][2]=attr;nodes.append(mod);m['core']=len(nodes)-1
 (out/f'{i}.json').write_text(json.dumps(j))
(directory/'synthetic-paths.txt').write_text(''.join(str(out/f'{i}.json')+'\n'for i in range(len(cases))))
print(len(cases))
