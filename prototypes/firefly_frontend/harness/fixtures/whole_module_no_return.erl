-module(whole_module_no_return).
-export([entry/1]).
entry(X) -> helper(X).
helper(X) -> error(X).
unused(X) -> X.
