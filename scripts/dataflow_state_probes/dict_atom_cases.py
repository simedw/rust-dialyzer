import json,random
r=random.Random(4163);atoms=['top','external','null','true','false','nil','ä','é','ÿ','😀','界','a'*255,'λ'*255]
for i in range(3000):
 count=r.choice([0,1,15,16,79,80,81,82,159,160,161,162,320,500,1000]);keys=[r.choice([r.randrange(100000),r.randrange(2**64),r.choice(atoms)])for _ in range(count)]
 # Exercise external insertion changing bucket expansions even when later excluded.
 if i%2==0:keys.insert(r.randrange(len(keys)+1),'external')
 print(json.dumps(dict(id=i,insert=keys)))
