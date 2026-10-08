-module(whole_module_anonymous).
-export([entry/1]).
entry(X) -> fun(Y) -> helper({X,Y}) end.
helper(X) -> X.
unused(X) -> X.
