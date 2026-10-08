-module(whole_module_local_narrow).
-export([entry/1]).
entry(X) -> helper(X).
helper(X) when is_integer(X) -> X + 1.
unused(X) -> X.
