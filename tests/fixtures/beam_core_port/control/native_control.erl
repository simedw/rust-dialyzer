-module(native_control).
-export([recursive/0,nonrecursive/0,capture/1,shadow/1,ignored/0,default/1,explicit/1,last/1,multiple/2,normal/1,no_match/1]).
recursive() -> fun F(0) -> 1; F(N) -> N*F(N-1) end.
nonrecursive() -> fun F(X) -> X end.
capture(A) -> fun F(0) -> A; F(N) -> F(N-1) end.
shadow(X) -> fun F(X) -> F(X) end.
ignored() -> fun _(X) -> X end.
default(A) -> maybe {ok,X} ?= A, X end.
explicit(A) -> maybe {ok,X} ?= A, X else E -> {error,E} end.
last(A) -> maybe {ok,X} ?= A end.
multiple(A,B) -> maybe {ok,X} ?= A, {ok,Y} ?= B, {X,Y} else {error,E} -> {bad,E}; Other -> Other end.
normal(A) -> maybe {ok,X} ?= A, Y=X+1, Y end.
no_match(A) -> maybe A+1 end.
