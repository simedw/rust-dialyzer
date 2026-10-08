import json,random,struct
from cache_fixture_lib import A,arena
r=random.Random(844);rows=[]
vals=[[],(),[1,2],(A('a'),A('b')),[[],(),(1,2)],A('else'),A('maybe'),A('and'),A('café'),A('中文'),A('a\0b'),0,1<<200,-(1<<200)]
for v in vals:
 for enc in ['latin1','unicode']:rows.append(dict(arena=arena(v),encoding=enc))
for _ in range(2000):
 v=(A(r.choice(['x','missing','has space','true','ßfoo','中文'])),r.getrandbits(80),[r.randint(-30,30),[],(A('foo'),r.getrandbits(30))],{A('a'):r.getrandbits(20),A('b'):(1,2)})
 for enc in ['latin1','unicode']:rows.append(dict(arena=arena(v),encoding=enc))
for _ in range(2000):
 bits=r.getrandbits(64)
 if ((bits>>52)&2047)==2047:bits&=~(2047<<52)
 rows.append(dict(arena=dict(root=0,terms=[dict(kind='float',value=f'{bits:016X}')]),encoding='latin1'))
for bits in range(65):
 import base64
 bs=bytearray(range((bits+7)//8))
 if bits%8:bs[-1]&=(255<<(8-bits%8))&255
 rows.append(dict(arena=dict(root=0,terms=[dict(kind='bitstring',bits=bits,bytes=base64.b64encode(bs).decode())]),encoding='latin1'))
for n in range(33):
 for _ in range(20):
  keys=r.sample(range(-100,100),n)
  rows.append(dict(arena=arena({k:(A('v'),k)for k in keys}),encoding='latin1'))
for _ in range(500):
 tail=r.choice([A('tail'),(A('tuple'),r.randint(1,100))]);a=arena(tail);ts=a['terms'];tail_id=a['root'];ts.append(dict(kind='integer',value=str(r.getrandbits(64))));ts.append(dict(kind='list',items=[len(ts)-1],tail=tail_id));a['root']=len(ts)-1;rows.append(dict(arena=a,encoding='latin1'))
open('form_io_commands.jsonl','w').writelines(json.dumps(c)+'\n' for c in rows)
