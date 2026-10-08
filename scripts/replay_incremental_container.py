#!/usr/bin/env python3
import argparse,hashlib,json,pathlib,subprocess,os
p=argparse.ArgumentParser();p.add_argument('--root',required=True);p.add_argument('--expected',required=True);p.add_argument('--out',required=True);a=p.parse_args()
root=pathlib.Path(a.root).resolve();expected=pathlib.Path(a.expected).resolve();out=pathlib.Path(a.out).resolve();out.mkdir(parents=True,exist_ok=False)
binary=root/'target/release/raw_incremental_pipeline_probe';profile=pathlib.Path('/Users/simedw/Documents/ChatGPT/rust-dialyzer/.cache/constraints/closure-code-profile.json');prior=pathlib.Path('/private/tmp/rust-dialyzer-import-lifecycle-checkpoint-1/proof/prior.json');reader=root/'scripts/incremental_container_reader.exs'
rows=[json.loads(x)for x in (expected/'requests.jsonl').read_text().splitlines()];assert len(rows)==133
paths=[binary,profile,prior,reader,expected/'requests.jsonl',*[p for p in (root/'src').rglob('*')if p.is_file()],root/'Cargo.toml',root/'Cargo.lock']
paths.extend(expected/(r['id']+'.native.iplt')for r in rows)
for library,version,mods in [('dialyzer','5.4',['dialyzer_iplt','dialyzer_plt']),('stdlib','7.2',['dict','maps','sets'])]:
 for m in mods:
  for folder,suffix in [('src','erl'),('ebin','beam')]:paths.append(pathlib.Path(f'/Users/simedw/.asdf/installs/erlang/28.3/lib/{library}-{version}/{folder}/{m}.{suffix}'))
def digest(p):
 h=hashlib.sha256()
 with p.open('rb')as f:
  for chunk in iter(lambda:f.read(1048576),b''):h.update(chunk)
 return h.hexdigest()
def inventory():return {str(p):digest(p)for p in sorted(set(paths))}
before=inventory();identity=hashlib.sha256(json.dumps(before,sort_keys=True).encode()).hexdigest()
for r in rows:r['out']=str(out/(r['id']+'.iplt'))
requests=out/'requests.jsonl';requests.write_text(''.join(json.dumps(r)+'\n'for r in rows))
for kind,n in [('raw',43),('warm',86),('named',4)]:
 source=[json.loads(x)for x in (expected/(kind+'.jsonl')).read_text().splitlines()];assert len(source)==n
 for r in source:r['out']=str(out/(r['id']+'.iplt'))
 data=''.join(json.dumps(r)+'\n'for r in source).encode()
 with (out/(kind+'.stderr')).open('wb')as err:
  run=subprocess.run([str(binary),str(profile),identity,*([str(prior)]if kind=='named'else[])],input=data,stdout=subprocess.PIPE,stderr=err,check=True)
 (out/(kind+'.rust.jsonl')).write_bytes(run.stdout);actual=list(map(json.loads,run.stdout.splitlines()));assert len(actual)==n and all(x['written']is True for x in actual)
env=dict(os.environ);env['PATH']='/Users/simedw/.asdf/installs/erlang/28.3/bin:/Users/simedw/.asdf/installs/elixir/1.19.4-otp-28/bin:'+env['PATH']
with (out/'reader.stderr').open('wb')as err:
 result=subprocess.run(['elixir',str(reader),str(out),str(expected),str(requests)],stdout=subprocess.PIPE,stderr=err,env=env,check=True)
(out/'reader.jsonl').write_bytes(result.stdout);actual=list(map(json.loads,result.stdout.splitlines()));assert len(actual)==133 and all(all(x[k]is True for k in ['container','native_reader','callback_reload','cache','types_logical'])for x in actual)
after=inventory();assert before==after
report={'cases':133,'exact':133,'reader':'Original dialyzer_iplt.plt_and_info_from_file','types':'logical equality','compressed_bytes':'not compared','public_incremental_workflows':'not exercised'}
(out/'report.json').write_text(json.dumps(report,indent=2)+'\n');(out/'provenance.json').write_text(json.dumps({'before':before,'after':after,'unchanged':True},indent=2)+'\n');print(json.dumps(report))
