-module(core_lower_after).
-export([small/1, large/1, nested/1]).
small(F) -> try F() after erlang:put(key,done) end.
large(F) -> try F() after put(a,1),put(b,2),put(c,3),put(d,4),put(e,5),put(f,6),put(g,7),put(h,8),put(i,9),put(j,10),put(k,11),put(l,12),put(m,13),put(n,14),put(o,15),put(p,16),put(q,17),put(r,18),put(s,19),put(t,20),put(u,21) end.
nested(F) -> try F() of ok -> success catch error:badarg -> failed after put(key,done) end.
