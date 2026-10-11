-module(core_lower_match).
-export([structural/1,last/1,repeat/1,pinned/2,last_literal/0]).
structural(F) -> {A,B} = F(), {B,A}.
last(F) -> {A,B} = F().
repeat({X,X}) -> X.
pinned(X,F) -> X = F(), X.
last_literal() -> {A,B} = {a,b}.
