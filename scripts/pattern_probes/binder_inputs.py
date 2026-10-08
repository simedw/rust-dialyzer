#!/usr/bin/env python3
"""Create binder requests from pinned manifest or same-VM native captures."""
import argparse,json,pathlib
p=argparse.ArgumentParser();p.add_argument('input');p.add_argument('--captured',action='store_true');p.add_argument('--output',required=True);a=p.parse_args()
text=pathlib.Path(a.input).read_text();rows=[json.loads(x)for x in text.splitlines()]if a.captured else json.loads(text)
ids=set()
with open(a.output,'w')as out:
 for row in rows:
  if row['id']in ids:raise ValueError('duplicate case identity')
  ids.add(row['id']);path=row['proof_core_json']if a.captured else row['core_json']
  if not pathlib.Path(path).is_file():raise ValueError('missing compiler proof input')
  out.write(json.dumps({'id':row['id'],'core_json':path})+'\n')
print(json.dumps({'cases':len(ids),'output':a.output}))
