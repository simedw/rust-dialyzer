import json,random
r=random.Random(1742)
for i in range(3000):
 size=r.choice([0,1,2,15,16,31,32,79,80,81,84,85,86,100,160,161,320,500,1000])
 keys=[r.choice([r.randrange(1000000),r.randrange(2**32),r.randrange(2**64),"top"]) for _ in range(size)]
 initial=[r.randrange(20) for _ in range(r.randrange(12))]
 operations=["pop" if r.randrange(3)==0 else {"add":r.choice(["top",r.randrange(20)])} for _ in range(r.randrange(40))]
 print(json.dumps(dict(id=i,insert=keys,initial=initial,operations=operations)))
