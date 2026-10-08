-module(whole_module_equality).
-export([entry/1]).
entry(X) -> case X =:= 1 of true -> one; false -> helper(X) end.
helper(X) -> X.
unused(X) -> X.
