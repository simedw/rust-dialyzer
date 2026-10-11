-module(native_exports).
-export([case_seq/1,case_set/1,case_tail/1,nested/1,pattern/1,receive_seq/0,receive_after/0,two/1,rebinding/1]).
case_seq(A) -> case A of a -> X=1; b -> X=2 end, X.
case_set(A) -> Y=case A of a -> X=1; b -> X=2 end, {Y,X}.
case_tail(A) -> case A of a -> X=1; b -> X=2 end.
nested(A) -> case A of a -> case A of a -> X=1; b -> X=2 end; b -> X=3 end, X.
pattern(A) -> case A of {a,X} -> X; {b,X} -> X end, X.
receive_seq() -> receive {a,X} -> X; {b,X} -> X end, X.
receive_after() -> receive {a,X} -> X after 0 -> X=42 end, X.
two(A) -> case A of a -> X=1,Y=2; b -> Y=3,X=4 end, {X,Y}.
rebinding(A) -> case A of a -> X=1; b -> X=2 end, X=1, X.
