import json
for depth,width in [(50000,1),(22,2)]:
 terms=[{'kind':'atom','value':a}for a in ['c','nominal_set','none','unknown','any']]+[{'kind':'nil'}]
 child=4
 for _ in range(depth):
  members=len(terms);terms.append({'kind':'list','items':[child]*width,'tail':5})
  elements=len(terms);terms.append({'kind':'tuple','items':[members,2]})
  child=len(terms);terms.append({'kind':'tuple','items':[0,1,elements,3]})
 print(json.dumps({'id':f'deep/{depth}/{width}','module':'here','raw':{'terms':terms,'root':child}},separators=(',',':')))
