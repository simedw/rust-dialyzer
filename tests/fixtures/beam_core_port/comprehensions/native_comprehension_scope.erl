-module(native_comprehension_scope).
-export([shadow/1,outer/2,nested/1,input/1]).
shadow(L) -> [fun(X) -> X end || X <- L].
outer(X,L) -> [fun(X) -> X end || X <- L].
nested(L) -> [fun(Y) -> {X,Y} end || X <- L, Y <- X].
input(L) -> [Y || X <- L, Y <- (fun(X) -> X end)(X)].
