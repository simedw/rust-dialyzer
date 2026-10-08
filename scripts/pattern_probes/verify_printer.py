#!/usr/bin/env python3
"""Strict native Core producer text comparison; unsupported output fails."""
import argparse,hashlib,json,pathlib,sys
p=argparse.ArgumentParser();p.add_argument('native_or_combined');p.add_argument('rust',nargs='?');p.add_argument('--report',default='data/printer_failures.json');a=p.parse_args()
def load(path):return [json.loads(x)for x in pathlib.Path(path).read_text().splitlines()]
x=load(a.native_or_combined);bad=[];modules=0;chars=0
if a.rust:
 y=load(a.rust)
 if len(x)!=len(y):raise ValueError('missing output rows')
 for n,r in zip(x,y):
  if n['id']!=r['id']or len(n['modules'])!=len(r['modules']):raise ValueError('case/module identities differ')
  for nm,rm in zip(n['modules'],r['modules']):
   modules+=1
   if nm['module']!=rm['module']:raise ValueError('module identities differ')
   if nm['text'].keys()!= {'Ok'}:raise ValueError('uncovered native exception')
   chars+=len(nm['text']['Ok'])
   if nm!=rm:bad.append({'id':n['id'],'module':nm['module'],'native':nm['text'],'rust':rm['text']})
else:
 for r in x:
  chars+=len(r['native'])
  if r['rust']!={'Ok':r['native']}:bad.append(r)
report={'rows':len(x),'modules':modules,'characters':chars,'mismatches':len(bad),'sha256':{s:hashlib.sha256(pathlib.Path(s).read_bytes()).hexdigest()for s in [a.native_or_combined,a.rust]if s}}
print(json.dumps(report,sort_keys=True))
if bad:pathlib.Path(a.report).write_text(json.dumps(bad,indent=2));sys.exit(1)
