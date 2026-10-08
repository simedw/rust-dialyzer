-module(whole_module_discarded_fun).
-export([entry/1]).
entry(X) -> _F = fun(Y) -> helper({X,Y}) end, ok.
helper(X) -> X.
unused(X) -> X.
