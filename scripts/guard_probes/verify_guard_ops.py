import hashlib,json,pathlib
root=pathlib.Path(__file__).resolve().parents[1]
files={name:root/'data'/f'guard_ops_{name}.jsonl' for name in ['cases','native','rust']}
rows={name:[json.loads(line) for line in path.read_text().splitlines()] for name,path in files.items()}
assert len(rows['cases'])==19020
assert len(rows['cases'])==len(rows['native'])==len(rows['rust'])
for case,native,rust in zip(rows['cases'],rows['native'],rows['rust']):
 assert case['id']==native['id']==rust['id']
 assert native==rust,(case,native,rust)
report={'status':'complete','cases':len(rows['cases']),'native_errors':sum('error'in r for r in rows['native']),'sha256':{name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in files.items()}}
(root/'data'/'guard_ops_report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
