-module(whole_module_recursion).
-export([entry/1]).
entry(X) -> helper(X).
helper(0) -> done; helper(X) when is_integer(X), X > 0 -> helper(X-1).
unused(X) -> X.
