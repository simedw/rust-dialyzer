-module(record_probe).
-export([make/1, get/1, update/2, info/0, test/1, guard/1, filter/1, capture/0, bits/1]).
-record(r,{a=foo,b={42,bar},c=undefined}).
make(X) -> #r{a=X}.
get(X) -> X#r.b.
update(X,Y) -> X#r{a=begin Y+1 end,c=Y}.
info() -> {#r.b,record_info(fields,r),record_info(size,r)}.
test(X) -> is_record(X,r).
guard(X) when X#r.a =:= foo, X#r.b =:= {42,bar} -> yes;
guard(_) -> no.
filter(Xs) -> [X#r.a || X <- Xs, X#r.a =:= foo].
capture() -> fun module_info/1.
bits(X) -> <<(X#r.a):8, (X#r.b):8>>.
